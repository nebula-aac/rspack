use std::any::Any;

use rkyv::{
  Archive, Deserialize, access,
  api::{deserialize_using, high::HighValidator},
  bytecheck::CheckBytes,
  de::Pool,
  rancor::{BoxedError, Source, Strategy, Trace},
  util::AlignedVec,
};

use crate::context::ContextGuard;

#[derive(Debug)]
pub enum DeserializeError {
  BoxedError(BoxedError),
  MessageError(&'static str),
  DynCheckBytesNotRegister,
  NoContext,
  UnsupportedField,
}

impl std::fmt::Display for DeserializeError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::BoxedError(error) => error.fmt(f),
      Self::MessageError(msg) => {
        write!(f, "{msg}")
      }
      Self::DynCheckBytesNotRegister => {
        write!(f, "cacheable_dyn check bytes not register")
      }
      Self::NoContext => {
        write!(f, "no context")
      }
      Self::UnsupportedField => {
        write!(f, "unsupported field")
      }
    }
  }
}

impl std::error::Error for DeserializeError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      Self::BoxedError(error) => error.source(),
      _ => None,
    }
  }
}

impl Trace for DeserializeError {
  fn trace<R>(self, trace: R) -> Self
  where
    R: std::fmt::Debug + std::fmt::Display + Send + Sync + 'static,
  {
    Self::BoxedError(BoxedError::trace(BoxedError::new(self), trace))
  }
}

impl Source for DeserializeError {
  fn new<T: std::error::Error + Send + Sync + 'static>(source: T) -> Self {
    Self::BoxedError(BoxedError::new(source))
  }
}

pub type Validator<'a> = HighValidator<'a, DeserializeError>;
pub type Deserializer = Strategy<Pool, DeserializeError>;

/// Transform bytes to struct
///
/// This function implementation refers to rkyv::from_bytes and
/// add custom error and context support
pub fn from_bytes<T, C: Any>(bytes: &[u8], context: &C) -> Result<T, DeserializeError>
where
  T: Archive,
  T::Archived: for<'a> CheckBytes<Validator<'a>> + Deserialize<T, Deserializer>,
{
  let guard = ContextGuard::new(context);
  let mut deserializer = Pool::default();
  guard.add_to_pooling(&mut deserializer)?;
  // The `bytes` ptr address in miri will throw UnalignedPointer error in rkyv.
  // AlignedVec will force aligned the ptr address.
  // Refer code: https://github.com/rkyv/rkyv/blob/dabbc1fcf5052f141403b84493bddb74c44f9ba9/rkyv/src/validation/archive/validator.rs#L135
  let mut aligned_vec = AlignedVec::<16>::new();
  aligned_vec.extend_from_slice(bytes);
  deserialize_using(
    access::<T::Archived, DeserializeError>(&aligned_vec)?,
    &mut deserializer,
  )
}
