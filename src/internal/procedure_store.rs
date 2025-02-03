use std::{borrow::Cow, collections::BTreeMap, future::ready, pin::Pin};

use futures::{stream::once, Stream};
use serde_json::Value;
use specta::DataType;
use specta_datatype_from::DataTypeFrom;

use crate::ExecError;

use super::{Layer, RequestContext, ValueOrStream};

#[derive(Debug, Clone, DataTypeFrom)]
#[cfg_attr(test, derive(specta::Type))]
#[cfg_attr(test, specta(rename = "ProcedureDef"))]
pub struct ProcedureDataType {
    pub key: Cow<'static, str>,
    #[specta(type = serde_json::Value)]
    pub input: DataType,
    #[specta(type = serde_json::Value)]
    pub result: DataType,
}

// TODO: Remove this type once v1
pub enum EitherLayer<TCtx> {
    Legacy(Box<dyn Layer<TCtx>>),
    #[cfg(feature = "alpha")]
    Alpha(Box<dyn crate::alpha::DynLayer<TCtx>>),
}

impl<TCtx: Send + 'static> EitherLayer<TCtx> {
    pub async fn call<'a>(
        &'a self,
        ctx: TCtx,
        input: Value,
        req: RequestContext,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Value, ExecError>> + Send + 'a>>, ExecError> {
        match self {
            // This is 100% going to tank legacy performance and I don't care
            Self::Legacy(l) => match l.call(ctx, input, req)?.into_value_or_stream().await? {
                ValueOrStream::Value(v) => Ok(Box::pin(once(ready(Ok(v))))),
                ValueOrStream::Stream(s) => Ok(Box::pin(s)),
            },
            #[cfg(feature = "alpha")]
            Self::Alpha(a) => Ok(a.dyn_call(ctx, input, req)),
        }
    }
}

// TODO: Make private
pub struct Procedure<TCtx> {
    // TODO: make private -> without breaking Spacedrive
    pub exec: EitherLayer<TCtx>,
    // TODO: make private -> without breaking Spacedrive
    pub ty: ProcedureDataType,
}

// TODO: make private
pub struct ProcedureStore<TCtx> {
    name: &'static str,
    // TODO: A `HashMap` would probs be best but due to const context's that is hard.
    pub(crate) store: BTreeMap<String, Procedure<TCtx>>,
}

impl<TCtx> ProcedureStore<TCtx> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            store: BTreeMap::new(),
        }
    }

    pub fn append(&mut self, key: String, exec: Box<dyn Layer<TCtx>>, ty: ProcedureDataType) {
        #[allow(clippy::panic)]
        if key.is_empty() || key == "ws" || key.starts_with("rpc.") || key.starts_with("rspc.") {
            panic!(
                "rspc error: attempted to create {} operation named '{}', however this name is not allowed.",
                self.name,
                key
            );
        }

        #[allow(clippy::panic)]
        if self.store.contains_key(&key) {
            panic!(
                "rspc error: {} operation already has resolver with name '{}'",
                self.name, key
            );
        }

        self.store.insert(
            key,
            Procedure {
                exec: EitherLayer::Legacy(exec),
                ty,
            },
        );
    }

    #[cfg(feature = "alpha")]
    pub(crate) fn append_alpha<L: crate::alpha::AlphaLayer<TCtx>>(
        &mut self,
        key: String,
        exec: L,
        ty: ProcedureDataType,
    ) where
        // TODO: move this bound to impl once `alpha` stuff is stable
        TCtx: 'static,
    {
        #[allow(clippy::panic)]
        if key.is_empty() || key == "ws" || key.starts_with("rpc.") || key.starts_with("rspc.") {
            panic!(
                "rspc error: attempted to create {} operation named '{}', however this name is not allowed.",
                self.name,
                key
            );
        }

        #[allow(clippy::panic)]
        if self.store.contains_key(&key) {
            panic!(
                "rspc error: {} operation already has resolver with name '{}'",
                self.name, key
            );
        }

        self.store.insert(
            key,
            Procedure {
                exec: EitherLayer::Alpha(exec.erase()),
                ty,
            },
        );
    }
}
