use crate::error::Error;
use heed::BoxedError;
use heed::BytesDecode;
use heed::BytesEncode;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::marker::PhantomData;

pub struct Bincode<T>(PhantomData<T>);

impl<'a, T> BytesEncode<'a> for Bincode<T>
where
    T: Serialize + 'a,
{
    type EItem = T;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        bincode::serialize(item)
            .map(Cow::Owned)
            .map_err(|e| Box::new(Error::Encode(e)) as BoxedError)
    }
}

impl<'a, T> BytesDecode<'a> for Bincode<T>
where
    T: Deserialize<'a> + 'a,
{
    type DItem = T;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        bincode::deserialize(bytes).map_err(|e| Box::new(Error::Encode(e)) as BoxedError)
    }
}
