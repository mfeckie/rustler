extern crate libc;
use self::libc::{ c_void };

use std::mem;
use std::ptr;
use std::marker::PhantomData;

use super::{ NifTerm, NifEnv, NifError, NifEncoder, NifDecoder };
use ::wrapper::nif_interface::{ NIF_RESOURCE_TYPE, NIF_RESOURCE_HANDLE, NIF_ENV, NifResourceFlags };

pub struct NifStructResourceType<T> {
    pub res: NIF_RESOURCE_TYPE,
    pub struct_type: PhantomData<T>,
}

pub trait NifResourceStruct: Sized {
    fn get_dtor() -> extern "C" fn(env: NIF_ENV, handle: NIF_RESOURCE_HANDLE);
    fn get_type<'a>() -> &'a NifStructResourceType<Self>;
    unsafe fn set_type(typ: NifStructResourceType<Self>);
}

impl<'b, T> NifEncoder for ResourceTypeHolder<'b, T> where T: NifResourceStruct+'b {
    fn encode<'a>(&self, env: &'a NifEnv) -> NifTerm<'a> {
        self.as_term(env)
    }
}
impl<'a, T> NifDecoder<'a> for ResourceTypeHolder<'a, T> where T: NifResourceStruct+'a {
    fn decode(term: NifTerm<'a>, env: &NifEnv) -> Result<Self, NifError> {
        ResourceTypeHolder::from_term(env, term)
    }
}

pub fn open_struct_resource_type<T: NifResourceStruct>(env: &NifEnv, name: &str,
                                 flags: NifResourceFlags) -> Option<NifStructResourceType<T>> {
    let res: Option<NIF_RESOURCE_TYPE> = ::wrapper::resource::open_resource_type(env.as_c_arg(), name, 
                                                                                 Some(T::get_dtor()), flags);
    if res.is_some() {
        Some(NifStructResourceType {
            res: res.unwrap(),
            struct_type: PhantomData,
        })
    } else {
        None
    }
}

pub fn get_alloc_size_struct<T>() -> usize {
    mem::size_of::<T>() + mem::align_of::<T>()
}

// This is unsafe as it allows us to do nasty things with pointers
pub unsafe fn align_alloced_mem_for_struct<T>(ptr: *mut c_void) -> *mut c_void {
    let offset = mem::align_of::<T>() - ((ptr as usize) % mem::align_of::<T>());
    ptr.offset(offset as isize)
}

use std::sync::RwLock;

pub struct ResourceTypeHolder<'a, T> where T: NifResourceStruct {
    raw: *mut c_void,
    inner: *mut RwLock<T>,
    env_life: PhantomData<&'a NifEnv>
}

use std::sync::{LockResult, RwLockReadGuard, RwLockWriteGuard};
impl<'a, T> ResourceTypeHolder<'a, T> where T: NifResourceStruct {
    pub fn new(_env: &'a NifEnv, data: T) -> Self {
        let alloc_size = get_alloc_size_struct::<RwLock<T>>();
        let mem_raw = ::wrapper::resource::alloc_resource(T::get_type().res, alloc_size);
        let aligned_mem = unsafe { align_alloced_mem_for_struct::<RwLock<T>>(mem_raw) } as *mut RwLock<T>;
        unsafe { ptr::write(aligned_mem, RwLock::new(data)) };
        ResourceTypeHolder {
            raw: mem_raw,
            inner: aligned_mem,
            env_life: PhantomData,
        }
    }
    fn from_term(env: &NifEnv, term: NifTerm<'a>) -> Result<Self, NifError> {
        let res_resource = match ::wrapper::resource::get_resource(env.as_c_arg(), term.as_c_arg(), T::get_type().res) {
            Some(res) => res,
            None => return Err(NifError::BadArg),
        };
        ::wrapper::resource::keep_resource(res_resource);
        let casted_ptr = unsafe { align_alloced_mem_for_struct::<RwLock<T>>(res_resource) } as *mut RwLock<T>;
        Ok(ResourceTypeHolder {
            raw: res_resource,
            inner: casted_ptr,
            env_life: PhantomData,
        })
    }

    fn as_term<'b>(&self, env: &'b NifEnv) -> NifTerm<'b> {
        NifTerm::new(env, ::wrapper::resource::make_resource(env.as_c_arg(), self.raw))
    }

    fn as_c_arg(&mut self) -> *mut c_void {
        self.raw
    }

    pub fn read(&self) -> LockResult<RwLockReadGuard<T>> {
        self.get_rwlock().read()
    }
    pub fn write(&self) -> LockResult<RwLockWriteGuard<T>> {
        self.get_rwlock().write()
    }

    pub fn get_rwlock(&self) -> &RwLock<T> {
        unsafe { &*self.inner }
    }
}

impl<'a, T> Drop for ResourceTypeHolder<'a, T> where T: NifResourceStruct {
    fn drop(&mut self) {
        unsafe { ::wrapper::nif_interface::enif_release_resource(self.as_c_arg()) };
    }
}