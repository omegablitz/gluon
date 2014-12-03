use std::ptr::RawPtr;
use std::mem;
use std::ptr;
use std::rt::heap::{allocate, deallocate};
use std::cell::RefCell;


pub struct Gc<T> {
    gc: RefCell<Gc_<T>>
}
struct Gc_<T> {
    values: Option<AllocPtr>,
    allocated_objects: uint,
    collect_limit: uint
}

struct GcHeader {
    next: Option<AllocPtr>,
    value_size: uint,
    marked: bool,
}


struct AllocPtr {
    ptr: *mut GcHeader
}

impl AllocPtr {
    fn new(value_size: uint) -> AllocPtr {
        unsafe {
            let ptr = allocate(GcHeader::value_offset() + value_size, mem::align_of::<f64>());
            let ptr: *mut GcHeader = mem::transmute(ptr);
            ptr::write(ptr, GcHeader { next: None, value_size: value_size, marked: false });
            AllocPtr { ptr: ptr }
        }
    }
}

#[unsafe_destructor]
impl Drop for AllocPtr {
    
    fn drop(&mut self) {
        unsafe {
            ptr::read(&*self.ptr);
            let size = GcHeader::value_offset() + self.value_size;
            deallocate(mem::transmute::<*mut GcHeader, *mut u8>(self.ptr), size, mem::align_of::<f64>());
        }
    }
}

impl Deref<GcHeader> for AllocPtr {
    fn deref(&self) -> &GcHeader {
        unsafe { & *self.ptr }
    }
}

impl DerefMut<GcHeader> for AllocPtr {
    fn deref_mut(&mut self) -> &mut GcHeader {
        unsafe { &mut *self.ptr }
    }
}

impl GcHeader {

    fn value(&self) -> *mut () {
        unsafe {
            let ptr: *mut u8 = mem::transmute(self);
            ptr.offset(GcHeader::value_offset() as int) as *mut ()
        }
    }

    fn value_offset() -> uint {
        let hs = mem::size_of::<GcHeader>();
        let max_align = mem::align_of::<f64>();
        hs + ((max_align - (hs % max_align)) % max_align)
    }

    fn total_size<T>() -> uint {
        GcHeader::value_offset() + mem::size_of::<T>()
    }
}


pub struct GcPtr<T> {
    ptr: *mut T
}

impl <T> Deref<T> for GcPtr<T> {
    fn deref(&self) -> &T {
        unsafe { & *self.ptr }
    }
}

impl <T> DerefMut<T> for GcPtr<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }
}

pub trait Traverseable<T> {
    fn traverse(&mut self, func: |&mut T|);
}

impl <T: Traverseable<T>> Gc<T> {

    pub fn new() -> Gc<T> {
        Gc { gc: RefCell::new(Gc_ { values: None, allocated_objects: 0, collect_limit: 100 }) }
    }
    pub fn alloc_and_collect<R: Traverseable<T>>(&self, roots: &mut R, value: T) -> GcPtr<T> {
        let ptr = self.gc.borrow_mut().alloc_and_collect(roots, value);
        GcPtr { ptr: ptr }
    }
    pub fn alloc(&self, value: T) -> GcPtr<T> {
        let ptr = self.gc.borrow_mut().alloc(value);
        GcPtr { ptr: ptr }
    }

    pub fn collect<R: Traverseable<T>>(&self, roots: &mut R) {
        self.gc.borrow_mut().collect(roots);
    }
}
impl <T: Traverseable<T>> Gc_<T> {
    
    fn alloc_and_collect<R: Traverseable<T>>(&mut self, roots: &mut R, value: T) -> *mut T {
        if self.allocated_objects >= self.collect_limit {
            self.collect(roots);
        }
        self.alloc(value)
    }
    fn alloc(&mut self, value: T) -> *mut T {
        let mut ptr = AllocPtr::new(mem::size_of::<T>());
        ptr.next = self.values.take();
        self.allocated_objects += 1;
        unsafe {
            let p: *mut T = mem::transmute(ptr.value());
            ptr::write(p, value);
            self.values = Some(ptr);
            p
        }
    }

    fn collect<R: Traverseable<T>>(&mut self, roots: &mut R) {
        roots.traverse(|v| self.mark(v));
        self.sweep();
    }

    fn gc_header(value: &mut T) -> &mut GcHeader {
        unsafe {
            let p: *mut u8 = mem::transmute(&mut *value);
            let header = p.offset(-(GcHeader::value_offset() as int));
            mem::transmute(header)
        }
    }

    fn mark(&mut self, value: &mut T) {
        {
            let header = Gc_::gc_header(value);
            if header.marked {
                return
            }
            header.marked = true;
        }
        value.traverse(|child| self.mark(child));
    }

    fn sweep(&mut self) {
        //Usage of unsafe are sadly needed to circumvent the borrow checker
        let mut first = self.values.take();
        {
            let mut maybe_header = &mut first;
            loop {
                let current: &mut Option<AllocPtr> = unsafe { mem::transmute(&*maybe_header) };
                maybe_header = match *maybe_header {
                    Some(ref mut header) => {
                        if !header.marked {
                            let unreached = mem::replace(current, header.next.take());
                            self.free(unreached);
                            continue
                        }
                        else {
                            header.marked = false;
                            let next: &mut Option<AllocPtr> = unsafe { mem::transmute(&mut header.next) };
                            next
                        }
                    }
                    None => break
                };
            }
        }
        self.values = first;
    }
    fn free(&mut self, header: Option<AllocPtr>) {
        self.allocated_objects -= 1;
        drop(header);
    }
}


#[cfg(test)]
mod tests {
    use super::{Gc, Gc_, GcPtr, GcHeader, Traverseable};
    use std::fmt;

    use self::Value::*;

    struct Data_ {
        fields: GcPtr<Vec<Value>>
    }
    impl  Deref<Vec<Value>> for Data_ {
        fn deref(&self) -> &Vec<Value> {
            &*self.fields
        }
    }
    impl  DerefMut<Vec<Value>> for Data_ {
        fn deref_mut(&mut self) -> &mut Vec<Value> {
            &mut *self.fields
        }
    }
    impl  PartialEq for Data_ {
        fn eq(&self, other: &Data_) -> bool {
            self.fields.ptr == other.fields.ptr
        }
    }
    impl  fmt::Show for Data_ {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            self.fields.ptr.fmt(f)
        }
    }

    #[deriving(PartialEq, Show)]
    enum Value {
        Int(int),
        Data(Data_)
    }
    impl  Traverseable<Vec<Value>> for Vec<Value> {
        fn traverse(&mut self, func: |&mut Vec<Value>|) {
            for v in self.iter_mut() {
                match *v {
                    Data(ref mut data) => func(&mut **data),
                    _ => ()
                }
            }
        }
    }
    impl  Traverseable<Vec<Value>> for Value {
        fn traverse(&mut self, func: |&mut Vec<Value>|) {
            match *self {
                Data(ref mut data) => func(&mut **data),
                _ => ()
            }
        }
    }

    fn num_objects<T>(gc: &Gc<T>) -> uint {
        let gc = gc.gc.borrow();
        let mut header: &GcHeader = match gc.values {
            Some(ref x) => &**x,
            None => return 0
        };
        let mut count = 1;
        loop {
            match header.next {
                Some(ref ptr) => {
                    count += 1;
                    header = &**ptr;
                }
                None => break
            }
        }
        count
    }

    fn new_data(p: GcPtr<Vec<Value>>) -> Value {
        Data(Data_ { fields: p })
    }

    #[test]
    fn gc_header() {
        let gc: Gc<Vec<Value>> = Gc::new();
        let mut ptr = gc.alloc(Vec::new());
        let header: *mut _ = &mut *Gc_::gc_header(&mut *ptr);
        let other: *mut _ = &mut **gc.gc.borrow_mut().values.as_mut().unwrap();
        assert_eq!(header, other);
    }

    #[test]
    fn basic() {
        let gc: Gc<Vec<Value>> = Gc::new();
        let mut stack: Vec<Value> = Vec::new();
        stack.push(new_data(gc.alloc(vec![Int(1)])));
        let d2 = new_data(gc.alloc(vec![stack[0]]));
        stack.push(d2);
        assert_eq!(num_objects(&gc), 2);
        gc.collect(&mut stack);
        assert_eq!(num_objects(&gc), 2);
        match stack[0] {
            Data(ref data) => assert_eq!((**data)[0], Int(1)),
            _ => panic!()
        }
        match stack[1] {
            Data(ref data) => assert_eq!((**data)[0], stack[0]),
            _ => panic!()
        }
        stack.pop();
        stack.pop();
        gc.collect(&mut stack);
        assert_eq!(num_objects(&gc), 0);
    }
}