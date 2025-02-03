use std::{
    borrow::Cow,
    marker::PhantomData,
    panic::Location,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::Stream;
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use specta::Type;

use crate::{alpha::Executable2, internal::RequestContext, ExecError};

use super::{
    AlphaLayer, AlphaMiddlewareBuilderLikeCompat, AlphaRequestLayer, FutureMarker, IntoProcedure,
    IntoProcedureCtx, MissingResolver, MwV2, MwV2Result, MwV3, PinnedOption, PinnedOptionProj,
    ProcedureLike, RequestKind, RequestLayerMarker, ResolverFunction, StreamLayerMarker,
    StreamMarker,
};

// TODO: `.with` but only support BEFORE resolver is set by the user.

// TODO: Check metadata stores on this so plugins can extend it to do cool stuff
// TODO: Logical order for these generics cause right now it's random
// TODO: Rename `RMarker` so cause we use it at runtime making it not really a "Marker" anymore
// TODO: Use named struct fields
pub struct AlphaProcedure<R, RMarker, TMiddleware>(
    // Is `None` after `.build()` is called. `.build()` can't take `self` cause dyn safety.
    Option<R>,
    // Is `None` after `.build()` is called. `.build()` can't take `self` cause dyn safety.
    Option<TMiddleware>,
    RMarker,
)
where
    TMiddleware: AlphaMiddlewareBuilderLike;

impl<TMiddleware, R, RMarker> AlphaProcedure<R, RMarker, TMiddleware>
where
    TMiddleware: AlphaMiddlewareBuilderLike,
{
    pub fn new_from_resolver(k: RMarker, mw: TMiddleware, resolver: R) -> Self {
        Self(Some(resolver), Some(mw), k)
    }
}

impl<TCtx, TLayerCtx> AlphaProcedure<MissingResolver<TLayerCtx>, (), AlphaBaseMiddleware<TCtx>>
where
    TCtx: Send + Sync + 'static,
    TLayerCtx: Send + Sync + 'static,
{
    pub fn new_from_middleware<TMiddleware>(
        mw: TMiddleware,
    ) -> AlphaProcedure<MissingResolver<TLayerCtx>, (), TMiddleware>
    where
        TMiddleware: AlphaMiddlewareBuilderLike<Ctx = TCtx>,
    {
        AlphaProcedure(Some(MissingResolver::default()), Some(mw), ())
    }
}

impl<TMiddleware> AlphaProcedure<MissingResolver<TMiddleware::LayerCtx>, (), TMiddleware>
where
    TMiddleware: AlphaMiddlewareBuilderLike,
{
    pub fn query<R, RMarker>(
        mut self,
        builder: R,
    ) -> AlphaProcedure<R, RequestLayerMarker<RMarker>, TMiddleware>
    where
        R: ResolverFunction<RequestLayerMarker<RMarker>, LayerCtx = TMiddleware::LayerCtx>
            + Fn(TMiddleware::LayerCtx, R::Arg) -> R::Result,
        R::Result: AlphaRequestLayer<R::RequestMarker, Type = FutureMarker>,
    {
        AlphaProcedure::new_from_resolver(
            RequestLayerMarker::new(RequestKind::Query),
            self.1.take().expect("Cannot configure procedure twice"),
            builder,
        )
    }

    pub fn mutation<R, RMarker>(
        mut self,
        builder: R,
    ) -> AlphaProcedure<R, RequestLayerMarker<RMarker>, TMiddleware>
    where
        R: ResolverFunction<RequestLayerMarker<RMarker>, LayerCtx = TMiddleware::LayerCtx>
            + Fn(TMiddleware::LayerCtx, R::Arg) -> R::Result,
        R::Result: AlphaRequestLayer<R::RequestMarker, Type = FutureMarker>,
    {
        AlphaProcedure::new_from_resolver(
            RequestLayerMarker::new(RequestKind::Mutation),
            self.1.take().expect("Cannot configure procedure twice"),
            builder,
        )
    }

    pub fn subscription<R, RMarker>(
        mut self,
        builder: R,
    ) -> AlphaProcedure<R, StreamLayerMarker<RMarker>, TMiddleware>
    where
        R: ResolverFunction<StreamLayerMarker<RMarker>, LayerCtx = TMiddleware::LayerCtx>
            + Fn(TMiddleware::LayerCtx, R::Arg) -> R::Result,
        R::Result: AlphaRequestLayer<R::RequestMarker, Type = StreamMarker>,
    {
        AlphaProcedure::new_from_resolver(
            StreamLayerMarker::new(),
            self.1.take().expect("Cannot configure procedure twice"),
            builder,
        )
    }
}

impl<TMiddleware> AlphaProcedure<MissingResolver<TMiddleware::LayerCtx>, (), TMiddleware>
where
    TMiddleware: AlphaMiddlewareBuilderLike + Sync,
{
    pub fn with<Mw: MwV2<TMiddleware::LayerCtx>>(
        self,
        mw: Mw,
    ) -> AlphaProcedure<MissingResolver<Mw::NewCtx>, (), AlphaMiddlewareLayerBuilder<TMiddleware, Mw>>
    {
        AlphaProcedure::new_from_middleware(AlphaMiddlewareLayerBuilder {
            middleware: self.1.expect("Cannot configure procedure twice"),
            mw,
        })
    }

    #[cfg(feature = "unstable")]
    pub fn with2<Mw: super::MwV3<TMiddleware::LayerCtx>>(
        self,
        mw: Mw,
    ) -> AlphaProcedure<MissingResolver<Mw::NewCtx>, (), AlphaMiddlewareLayerBuilder<TMiddleware, Mw>>
    {
        AlphaProcedure::new_from_middleware(AlphaMiddlewareLayerBuilder {
            middleware: self.1.expect("Cannot configure procedure twice"),
            mw,
        })
    }
}

impl<R, RMarker, TMiddleware> IntoProcedure<TMiddleware::Ctx>
    for AlphaProcedure<R, RequestLayerMarker<RMarker>, TMiddleware>
where
    R: ResolverFunction<RequestLayerMarker<RMarker>, LayerCtx = TMiddleware::LayerCtx>,
    RMarker: 'static,
    R::Result: AlphaRequestLayer<R::RequestMarker, Type = FutureMarker>,
    TMiddleware: AlphaMiddlewareBuilderLike,
{
    #[track_caller]
    fn build(&mut self, key: Cow<'static, str>, ctx: &mut IntoProcedureCtx<'_, TMiddleware::Ctx>) {
        let (resolver, middleware) = self
            .0
            .take()
            .map(
                // TODO: Removing `Arc` by moving ownership to `AlphaResolverLayer`
                Arc::new,
            )
            .zip(self.1.take())
            .expect("Cannot call '.build()' multiple times!");

        let m = match self.2.kind() {
            RequestKind::Query => &mut ctx.queries,
            RequestKind::Mutation => &mut ctx.mutations,
        };

        m.append_alpha(
            key.to_string(),
            middleware.build(AlphaResolverLayer {
                func: move |ctx, input, _| {
                    Ok(resolver
                        .exec(
                            ctx,
                            serde_json::from_value(input)
                                .map_err(ExecError::DeserializingArgErr)?,
                        )
                        .exec())
                },
                phantom: PhantomData,
            }),
            R::typedef::<TMiddleware>(key, ctx.ty_store).unwrap_or_else(|_| {
                panic!(
                    "{}: Failed to generate type definition for procedure",
                    Location::caller()
                )
            }),
        );
    }
}

impl<R, RMarker, TMiddleware> IntoProcedure<TMiddleware::Ctx>
    for AlphaProcedure<R, StreamLayerMarker<RMarker>, TMiddleware>
where
    R: ResolverFunction<StreamLayerMarker<RMarker>, LayerCtx = TMiddleware::LayerCtx>,
    RMarker: 'static,
    R::Result: AlphaRequestLayer<R::RequestMarker, Type = StreamMarker>,
    TMiddleware: AlphaMiddlewareBuilderLike,
{
    #[track_caller]
    fn build(&mut self, key: Cow<'static, str>, ctx: &mut IntoProcedureCtx<'_, TMiddleware::Ctx>) {
        let (resolver, middleware) = self
            .0
            .take()
            .map(
                // TODO: Removing `Arc`?
                Arc::new,
            )
            .zip(self.1.take())
            .expect("Cannot call '.build()' multiple times!");

        ctx.subscriptions.append_alpha(
            key.to_string(),
            middleware.build(AlphaResolverLayer {
                func: move |ctx, input, _| {
                    Ok(resolver
                        .exec(
                            ctx,
                            serde_json::from_value(input)
                                .map_err(ExecError::DeserializingArgErr)?,
                        )
                        .exec())
                },
                phantom: PhantomData,
            }),
            R::typedef::<TMiddleware>(key, ctx.ty_store).unwrap_or_else(|_| {
                panic!(
                    "{}: Failed to generate type definition for procedure",
                    Location::caller()
                )
            }),
        );
    }
}

// TODO: This only works without a resolver. `ProcedureLike` should work on `AlphaProcedure` without it but just without the `.query()` and `.mutate()` functions.
impl<TMiddleware> ProcedureLike
    for AlphaProcedure<MissingResolver<TMiddleware::LayerCtx>, (), TMiddleware>
where
    TMiddleware: AlphaMiddlewareBuilderLike,
{
    type Middleware = TMiddleware;
    type LayerCtx = TMiddleware::LayerCtx;

    fn query<R, RMarker>(
        mut self,
        builder: R,
    ) -> AlphaProcedure<R, RequestLayerMarker<RMarker>, Self::Middleware>
    where
        R: ResolverFunction<RequestLayerMarker<RMarker>, LayerCtx = TMiddleware::LayerCtx>
            + Fn(TMiddleware::LayerCtx, R::Arg) -> R::Result,
        R::Result: AlphaRequestLayer<R::RequestMarker, Type = FutureMarker>,
    {
        AlphaProcedure::new_from_resolver(
            RequestLayerMarker::new(RequestKind::Query),
            self.1.take().expect("Cannot configure procedure twice"),
            builder,
        )
    }

    fn mutation<R, RMarker>(
        mut self,
        builder: R,
    ) -> AlphaProcedure<R, RequestLayerMarker<RMarker>, Self::Middleware>
    where
        R: ResolverFunction<RequestLayerMarker<RMarker>, LayerCtx = TMiddleware::LayerCtx>
            + Fn(TMiddleware::LayerCtx, R::Arg) -> R::Result,
        R::Result: AlphaRequestLayer<R::RequestMarker, Type = FutureMarker>,
    {
        AlphaProcedure::new_from_resolver(
            RequestLayerMarker::new(RequestKind::Query),
            self.1.take().expect("Cannot configure procedure twice"),
            builder,
        )
    }

    fn subscription<R, RMarker>(
        mut self,
        builder: R,
    ) -> AlphaProcedure<R, StreamLayerMarker<RMarker>, Self::Middleware>
    where
        R: ResolverFunction<StreamLayerMarker<RMarker>, LayerCtx = TMiddleware::LayerCtx>
            + Fn(TMiddleware::LayerCtx, R::Arg) -> R::Result,
        R::Result: AlphaRequestLayer<R::RequestMarker, Type = StreamMarker>,
    {
        AlphaProcedure::new_from_resolver(
            StreamLayerMarker::new(),
            self.1.take().expect("Cannot configure procedure twice"),
            builder,
        )
    }
}

///
/// `internal/middleware.rs`
///
use std::future::Future;

use serde_json::Value;

pub trait AlphaMiddlewareBuilderLike: Send + 'static {
    type Ctx: Send + Sync + 'static;
    type LayerCtx: Send + Sync + 'static;
    type Arg<T: Type + DeserializeOwned + 'static>: Type + DeserializeOwned + 'static;

    type LayerResult<T>: AlphaLayer<Self::Ctx>
    where
        T: AlphaLayer<Self::LayerCtx>;

    fn build<T>(self, next: T) -> Self::LayerResult<T>
    where
        T: AlphaLayer<Self::LayerCtx>;
}

impl<M: AlphaMiddlewareBuilderLike> AlphaMiddlewareBuilderLikeCompat for M {
    type Arg<T: Type + DeserializeOwned + 'static> = M::Arg<T>;
}

pub struct AlphaMiddlewareLayerBuilder<TMiddleware, TNewMiddleware>
where
    TMiddleware: AlphaMiddlewareBuilderLike,
    TNewMiddleware: MwV3<TMiddleware::LayerCtx>,
{
    pub(crate) middleware: TMiddleware,
    pub(crate) mw: TNewMiddleware,
}

impl<TLayerCtx, TMiddleware, TNewMiddleware> AlphaMiddlewareBuilderLike
    for AlphaMiddlewareLayerBuilder<TMiddleware, TNewMiddleware>
where
    TLayerCtx: Send + Sync + 'static,
    TMiddleware: AlphaMiddlewareBuilderLike<LayerCtx = TLayerCtx> + Send + Sync + 'static,
    TNewMiddleware: MwV3<TLayerCtx> + Send + Sync + 'static,
{
    type Ctx = TMiddleware::Ctx;
    type LayerCtx = TNewMiddleware::NewCtx;
    type LayerResult<T> = TMiddleware::LayerResult<AlphaMiddlewareLayer<TLayerCtx, T, TNewMiddleware>>
    where
        T: AlphaLayer<Self::LayerCtx>;
    type Arg<T: Type + DeserializeOwned + 'static> = TNewMiddleware::Arg<T>;

    fn build<T>(self, next: T) -> Self::LayerResult<T>
    where
        T: AlphaLayer<Self::LayerCtx> + Sync,
    {
        self.middleware.build(AlphaMiddlewareLayer {
            next,
            mw: self.mw,
            phantom: PhantomData,
        })
    }
}

pub struct AlphaMiddlewareLayer<TLayerCtx, TMiddleware, TNewMiddleware>
where
    TLayerCtx: Send + Sync + 'static,
    TMiddleware: AlphaLayer<TNewMiddleware::NewCtx> + Sync + 'static,
    TNewMiddleware: MwV3<TLayerCtx> + Send + Sync + 'static,
{
    next: TMiddleware,
    mw: TNewMiddleware,
    phantom: PhantomData<TLayerCtx>,
}

impl<TLayerCtx, TMiddleware, TNewMiddleware> AlphaLayer<TLayerCtx>
    for AlphaMiddlewareLayer<TLayerCtx, TMiddleware, TNewMiddleware>
where
    TLayerCtx: Send + Sync + 'static,
    TMiddleware: AlphaLayer<TNewMiddleware::NewCtx> + Sync + 'static,
    TNewMiddleware: MwV3<TLayerCtx> + Send + Sync + 'static,
{
    type Stream<'a> = MiddlewareFutOrSomething<'a, TLayerCtx, TNewMiddleware, TMiddleware>;

    fn call(
        &self,
        ctx: TLayerCtx,
        input: Value,
        req: RequestContext,
    ) -> Result<Self::Stream<'_>, ExecError> {
        let fut = self.mw.run_me(
            ctx,
            super::middleware::AlphaMiddlewareContext {
                input,
                req,
                _priv: (),
            },
        );

        Ok(MiddlewareFutOrSomething(
            PinnedOption::Some(fut),
            &self.next,
            PinnedOption::None,
            None,
            PinnedOption::None,
        ))
    }
}

// TODO: Rename this type
// TODO: Cleanup generics on this
// TODO: Use named fields!!!!!
#[pin_project(project = MiddlewareFutOrSomethingProj)]
pub struct MiddlewareFutOrSomething<
    'a,
    // TODO: Remove one of these Ctx's and get from `TMiddleware` or `TNextMiddleware`
    TLayerCtx: Send + Sync + 'static,
    TNewMiddleware: MwV3<TLayerCtx> + Send + Sync + 'static,
    TMiddleware: AlphaLayer<TNewMiddleware::NewCtx> + 'static,
>(
    #[pin] PinnedOption<TNewMiddleware::Fut>,
    &'a TMiddleware,
    #[pin] PinnedOption<TMiddleware::Stream<'a>>,
    Option<<TNewMiddleware::Result as MwV2Result>::Resp>,
    #[pin] PinnedOption<<<TNewMiddleware::Result as MwV2Result>::Resp as Executable2>::Fut>,
);

impl<
        'a,
        TLayerCtx: Send + Sync + 'static,
        TNewMiddleware: MwV3<TLayerCtx> + Send + Sync + 'static,
        TMiddleware: AlphaLayer<TNewMiddleware::NewCtx> + 'static,
    > Stream for MiddlewareFutOrSomething<'a, TLayerCtx, TNewMiddleware, TMiddleware>
{
    type Item = Result<Value, ExecError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        match this.0.as_mut().project() {
            PinnedOptionProj::Some(fut) => match fut.poll(cx) {
                Poll::Ready(result) => {
                    this.0.set(PinnedOption::None);

                    let (ctx, input, req, resp) = result.explode()?;
                    *this.3 = resp;

                    match this.1.call(ctx, input, req) {
                        Ok(stream) => this.2.set(PinnedOption::Some(stream)),
                        Err(err) => return Poll::Ready(Some(Err(err))),
                    }
                }
                Poll::Pending => return Poll::Pending,
            },
            PinnedOptionProj::None => {}
        }

        match this.4.as_mut().project() {
            PinnedOptionProj::Some(fut) => match fut.poll(cx) {
                Poll::Ready(result) => {
                    this.4.set(PinnedOption::None);

                    return Poll::Ready(Some(Ok(result)));
                }
                Poll::Pending => return Poll::Pending,
            },
            PinnedOptionProj::None => {}
        }

        match this.2.as_mut().project() {
            PinnedOptionProj::Some(fut) => match fut.poll_next(cx) {
                Poll::Ready(result) => match this.3.take() {
                    Some(resp) => {
                        let result = match result
                            .ok_or_else(|| ExecError::Internal("Empty result".to_string()))
                            .and_then(|result| result)
                        {
                            Ok(result) => result,
                            Err(err) => {
                                tracing::error!(
                                    "Failed to get result from subscription: {:?}",
                                    err
                                );
                                Value::Null
                            }
                        };

                        let fut = resp.call(result);
                        this.4.set(PinnedOption::Some(fut));
                    }
                    None => return Poll::Ready(result),
                },
                Poll::Pending => return Poll::Pending,
            },
            PinnedOptionProj::None => {}
        }

        unreachable!()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.2 {
            PinnedOption::Some(stream) => stream.size_hint(),
            PinnedOption::None => (0, None),
        }
    }
}

pub struct AlphaBaseMiddleware<TCtx>(PhantomData<TCtx>)
where
    TCtx: 'static;

impl<TCtx> Default for AlphaBaseMiddleware<TCtx>
where
    TCtx: 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<TCtx> AlphaBaseMiddleware<TCtx>
where
    TCtx: 'static,
{
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<TCtx> AlphaMiddlewareBuilderLike for AlphaBaseMiddleware<TCtx>
where
    TCtx: Send + Sync + 'static,
{
    type Ctx = TCtx;
    type LayerCtx = TCtx;

    type LayerResult<T> = T
    where
        T: AlphaLayer<Self::LayerCtx>;
    type Arg<T: Type + DeserializeOwned + 'static> = T;

    fn build<T>(self, next: T) -> Self::LayerResult<T>
    where
        T: AlphaLayer<Self::LayerCtx>,
    {
        next
    }
}

pub struct AlphaResolverLayer<TLayerCtx, T, S>
where
    TLayerCtx: Send + Sync + 'static,
    T: Fn(TLayerCtx, Value, RequestContext) -> Result<S, ExecError> + Send + Sync + 'static,
    S: Stream<Item = Result<Value, ExecError>> + Send + 'static,
{
    pub func: T,
    pub phantom: PhantomData<TLayerCtx>,
}

impl<T, TLayerCtx, S> AlphaLayer<TLayerCtx> for AlphaResolverLayer<TLayerCtx, T, S>
where
    TLayerCtx: Send + Sync + 'static,
    T: Fn(TLayerCtx, Value, RequestContext) -> Result<S, ExecError> + Send + Sync + 'static,
    S: Stream<Item = Result<Value, ExecError>> + Send + 'static,
{
    type Stream<'a> = S;

    fn call(
        &self,
        a: TLayerCtx,
        b: Value,
        c: RequestContext,
    ) -> Result<Self::Stream<'_>, ExecError> {
        (self.func)(a, b, c)
    }
}
