use std::future::Future;

use serde::de::DeserializeOwned;
use specta::Type;

use super::{AlphaMiddlewareContext, MwV2Result};

/// TODO
///
// Version of [MwV2] without the supertrait.
pub trait MwV3<TLCtx>: Send + Sync + 'static {
    type Fut: Future<Output = Self::Result> + Send + 'static;
    type Result: MwV2Result<Ctx = Self::NewCtx>;
    type NewCtx: Send + Sync + 'static;
    type Arg<T: Type + DeserializeOwned + 'static>: Type + DeserializeOwned + 'static;

    // TODO: Rename
    fn run_me(&self, ctx: TLCtx, mw: AlphaMiddlewareContext) -> Self::Fut;
}

/// TODO
///
// This must have the `Fn` supertrait, otherwise Rust will fail to infer `TLCtx` in userspace.
pub trait MwV2<TLCtx>:
    MwV3<TLCtx> + Fn(AlphaMiddlewareContext, TLCtx) -> Self::Fut + Send + Sync + 'static
where
    TLCtx: Send + Sync + 'static,
{
}

impl<TLCtx, F, Fu, R> MwV2<TLCtx> for F
where
    TLCtx: Send + Sync + 'static,
    F: Fn(AlphaMiddlewareContext, TLCtx) -> Fu + Send + Sync + 'static,
    Fu: Future<Output = R> + Send + 'static,
    R: MwV2Result + Send + 'static,
{
}

impl<TLCtx, F, Fu, R> MwV3<TLCtx> for F
where
    TLCtx: Send + Sync + 'static,
    F: Fn(AlphaMiddlewareContext, TLCtx) -> Fu + Send + Sync + 'static,
    Fu: Future<Output = R> + Send + 'static,
    R: MwV2Result + Send + 'static,
{
    type Fut = Fu;
    type Result = R;
    type NewCtx = R::Ctx; // TODO: Make this work with context switching
    type Arg<T: Type + DeserializeOwned + 'static> = T;

    fn run_me(&self, ctx: TLCtx, mw: AlphaMiddlewareContext) -> Self::Fut {
        self(mw, ctx)
    }
}
