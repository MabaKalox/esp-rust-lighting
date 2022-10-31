use std::sync::{Arc, LockResult, RwLock, RwLockReadGuard};

pub struct ReadOnly<T> {
    inner: Arc<RwLock<T>>,
}

impl<T> ReadOnly<T> {
    pub fn read(&self) -> LockResult<RwLockReadGuard<'_, T>> {
        self.inner.read()
    }
}

impl<T> Clone for ReadOnly<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

pub trait ToReadOnly<T> {
    fn read_only(self) -> ReadOnly<T>;
}

impl<T> ToReadOnly<T> for Arc<RwLock<T>> {
    fn read_only(self) -> ReadOnly<T> {
        ReadOnly { inner: self }
    }
}
