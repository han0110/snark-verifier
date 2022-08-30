use crate::{
    loader::{
        evm::{
            loader::{EcPoint, EvmLoader, Scalar, Value},
            u256_to_fe,
        },
        native::NativeLoader,
        Loader,
    },
    util::{Curve, Group, Itertools, PrimeField, Transcript, TranscriptRead, UncompressedEncoding},
    Error,
};
use ethereum_types::U256;
use sha3::{Digest, Keccak256};
use std::{
    io::{self, Read, Write},
    marker::PhantomData,
    rc::Rc,
};

pub struct EvmTranscript<C: Curve, L: Loader<C>, S, B> {
    loader: L,
    stream: S,
    buf: B,
    _marker: PhantomData<C>,
}

impl<C> EvmTranscript<C, Rc<EvmLoader>, usize, Vec<Value>>
where
    C: Curve + UncompressedEncoding<Uncompressed = [u8; 64]>,
    C::Scalar: PrimeField<Repr = [u8; 32]>,
{
    pub fn new(loader: Rc<EvmLoader>) -> Self {
        Self {
            loader,
            stream: 0,
            buf: Vec::new(),
            _marker: PhantomData,
        }
    }
}

impl<C> Transcript<C, Rc<EvmLoader>> for EvmTranscript<C, Rc<EvmLoader>, usize, Vec<Value>>
where
    C: Curve + UncompressedEncoding<Uncompressed = [u8; 64]>,
    C::Scalar: PrimeField<Repr = [u8; 32]>,
{
    fn squeeze_challenge(&mut self) -> Scalar {
        self.loader.squeeze_challenge(self.buf.drain(..).collect())
    }

    fn common_ec_point(&mut self, ec_point: &EcPoint) -> Result<(), Error> {
        self.buf.extend([ec_point.x(), ec_point.y()]);
        Ok(())
    }

    fn common_scalar(&mut self, scalar: &Scalar) -> Result<(), Error> {
        if self.stream == 0 {
            self.loader.set_transcript_state(scalar.value());
        } else {
            self.buf.push(scalar.value());
        }
        Ok(())
    }
}

impl<C> TranscriptRead<C, Rc<EvmLoader>> for EvmTranscript<C, Rc<EvmLoader>, usize, Vec<Value>>
where
    C: Curve + UncompressedEncoding<Uncompressed = [u8; 64]>,
    C::Scalar: PrimeField<Repr = [u8; 32]>,
{
    fn read_scalar(&mut self) -> Result<Scalar, Error> {
        let scalar = self.loader.calldataload_scalar(self.stream);
        self.stream += 0x20;
        self.common_scalar(&scalar)?;
        Ok(scalar)
    }

    fn read_ec_point(&mut self) -> Result<EcPoint, Error> {
        let ec_point = self.loader.calldataload_ec_point(self.stream);
        self.stream += 0x40;
        self.common_ec_point(&ec_point)?;
        Ok(ec_point)
    }
}

impl<C, S> EvmTranscript<C, NativeLoader, S, Vec<u8>>
where
    C: Curve,
{
    pub fn new(stream: S) -> Self {
        Self {
            loader: NativeLoader,
            stream,
            buf: Vec::new(),
            _marker: PhantomData,
        }
    }
}

impl<C, S> Transcript<C, NativeLoader> for EvmTranscript<C, NativeLoader, S, Vec<u8>>
where
    C: Curve + UncompressedEncoding<Uncompressed = [u8; 64]>,
    C::Scalar: PrimeField<Repr = [u8; 32]>,
{
    fn squeeze_challenge(&mut self) -> C::Scalar {
        let data = self
            .buf
            .iter()
            .cloned()
            .chain(if self.buf.len() == 0x20 {
                Some(1)
            } else {
                None
            })
            .collect_vec();
        let hash: [u8; 32] = Keccak256::digest(data).into();
        self.buf = hash.to_vec();
        u256_to_fe(U256::from_big_endian(hash.as_slice()))
    }

    fn common_ec_point(&mut self, ec_point: &C) -> Result<(), Error> {
        let uncopressed = ec_point.to_uncompressed();
        self.buf.extend(uncopressed[..32].iter().rev().cloned());
        self.buf.extend(uncopressed[32..].iter().rev().cloned());

        Ok(())
    }

    fn common_scalar(&mut self, scalar: &C::Scalar) -> Result<(), Error> {
        self.buf.extend(scalar.to_repr().as_ref().iter().rev());

        Ok(())
    }
}

impl<C, S> TranscriptRead<C, NativeLoader> for EvmTranscript<C, NativeLoader, S, Vec<u8>>
where
    C: Curve + UncompressedEncoding<Uncompressed = [u8; 64]>,
    C::Scalar: PrimeField<Repr = [u8; 32]>,
    S: Read,
{
    fn read_scalar(&mut self) -> Result<C::Scalar, Error> {
        let mut data = [0; 32];
        self.stream
            .read_exact(data.as_mut())
            .map_err(|err| Error::Transcript(err.kind(), err.to_string()))?;
        data.reverse();
        let scalar = <C as Group>::Scalar::from_repr_vartime(data).ok_or_else(|| {
            Error::Transcript(
                io::ErrorKind::Other,
                "Invalid scalar encoding in proof".to_string(),
            )
        })?;
        self.common_scalar(&scalar)?;
        Ok(scalar)
    }

    fn read_ec_point(&mut self) -> Result<C, Error> {
        let mut data = [0; 64];
        self.stream
            .read_exact(data.as_mut())
            .map_err(|err| Error::Transcript(err.kind(), err.to_string()))?;
        data.as_mut_slice()[..32].reverse();
        data.as_mut_slice()[32..].reverse();
        let ec_point = C::from_uncompressed(data).ok_or_else(|| {
            Error::Transcript(
                io::ErrorKind::Other,
                "Invalid elliptic curve point encoding in proof".to_string(),
            )
        })?;
        self.common_ec_point(&ec_point)?;
        Ok(ec_point)
    }
}

impl<C, S> EvmTranscript<C, NativeLoader, S, Vec<u8>>
where
    C: Curve,
    S: Write,
{
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    pub fn finalize(self) -> S {
        self.stream
    }
}
