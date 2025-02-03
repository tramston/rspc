use std::marker::PhantomData;

use serde_json::Value;

use crate::internal::RequestContext;

use super::{Executable2Placeholder, MwResultWithCtx};

// TODO: Deal with ambigious types cause two of them have this same name!
// TODO: Only hold output and not the whole `M` generic?
pub struct AlphaMiddlewareContext {
    pub input: Value,
    pub req: RequestContext,
    // Prevents downstream user constructing type
    pub(crate) _priv: (),
}

impl AlphaMiddlewareContext {
    pub fn next<TNCtx>(self, ctx: TNCtx) -> MwResultWithCtx<TNCtx, Executable2Placeholder> {
        MwResultWithCtx {
            input: self.input,
            req: self.req,
            ctx: Some(ctx),
            resp: None,
        }
    }
}

#[deprecated = "Maybe remove this type?"]
pub struct AlphaMiddlewareContext2<M> {
    phantom: PhantomData<M>,
}
