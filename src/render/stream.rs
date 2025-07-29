use futures::{
    stream::{FusedStream, StreamExt},
    Stream,
};
use pin_project_lite::pin_project;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

pin_project! {
    #[must_use = "streams do nothing unless polled"]
    pub struct Multiplexer<St, F> {
        #[pin]
        inner: Vec<St>,
        f: F
    }
}

pub fn multiplex<I, F>(streams: I, f: F) -> Multiplexer<I::Item, F>
where
    I: IntoIterator,
    I::Item: Stream + Unpin + FusedStream,
    F: FnMut() -> usize,
{
    let mut set = Vec::new();

    for stream in streams {
        set.push(stream);
    }

    Multiplexer { inner: set, f }
}

impl<St, F> Stream for Multiplexer<St, F>
where
    St: Stream + Unpin + FusedStream,
    F: FnMut() -> usize,
{
    type Item = St::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<<Self as Stream>::Item>> {
        let this = self.project();

        let index = (this.f)();
        let inner_vec = this.inner.get_mut();
        inner_vec
            .get_mut(index)
            .expect("Bad index")
            .poll_next_unpin(cx)
    }
}

impl<St, F> FusedStream for Multiplexer<St, F>
where
    St: Stream + Unpin + FusedStream,
    F: FnMut() -> usize,
{
    fn is_terminated(&self) -> bool {
        self.inner.iter().all(FusedStream::is_terminated)
    }
}

impl<St, F> Multiplexer<St, F>
where
    St: Stream + Unpin + FusedStream,
    F: FnMut() -> usize,
{
    #[allow(dead_code)]
    pub fn new(futures: Vec<St>, f: F) -> Self {
        Self { inner: futures, f }
    }
}
