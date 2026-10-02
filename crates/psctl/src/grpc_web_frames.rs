//! Keep gRPC-Web messages and trailers in separate HTTP body frames.
//!
//! tonic-web 0.14 drops trailers when the final message and trailers arrive in
//! one data frame (<https://github.com/grpc/grpc-rust/pull/2474>). Split at protocol
//! boundaries before decoding, independent of proxy/TCP packet boundaries.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use bytes::{Bytes, BytesMut};
use http::{Request, Response};
use hyper::body::{Body, Frame};
use tower::Service;

#[derive(Clone)]
pub struct ResponseFrames<S>(pub S);

impl<S, ReqBody, B> Service<Request<ReqBody>> for ResponseFrames<S>
where
    S: Service<Request<ReqBody>, Response = Response<B>>,
    S::Future: Send + 'static,
    B: 'static,
    S::Error: 'static,
{
    type Response = Response<GrpcWebFrames<B>>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let future = self.0.call(req);
        Box::pin(async move {
            future
                .await
                .map(|response| response.map(GrpcWebFrames::new))
        })
    }
}

pub struct GrpcWebFrames<B> {
    inner: B,
    pending: BytesMut,
}

impl<B> GrpcWebFrames<B> {
    fn new(inner: B) -> Self {
        Self {
            inner,
            pending: BytesMut::new(),
        }
    }
}

impl<B> Body for GrpcWebFrames<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: std::fmt::Display,
{
    type Data = Bytes;
    type Error = tonic::Status;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();

        loop {
            if let [_, a, b, c, d, ..] = this.pending.as_ref() {
                let len = u32::from_be_bytes([*a, *b, *c, *d]) as usize;

                // Match tonic's default maximum decoded message size. Do not
                // buffer an unbounded frame length supplied by a remote server.
                if len > 4 * 1024 * 1024 {
                    return Poll::Ready(Some(Err(tonic::Status::resource_exhausted(
                        "gRPC-Web frame exceeds 4 MiB",
                    ))));
                }
                if this.pending.len() >= len + 5 {
                    let frame = this.pending.split_to(len + 5).freeze();
                    return Poll::Ready(Some(Ok(Frame::data(frame))));
                }
            }

            match ready!(Pin::new(&mut this.inner).poll_frame(cx)) {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => this.pending.extend_from_slice(&data),
                    Err(frame) if this.pending.is_empty() => return Poll::Ready(Some(Ok(frame))),
                    Err(_) => {
                        return Poll::Ready(Some(Err(tonic::Status::unknown(
                            "truncated gRPC-Web frame",
                        ))));
                    }
                },
                Some(Err(error)) => {
                    return Poll::Ready(Some(Err(tonic::Status::unknown(error.to_string()))));
                }
                None if this.pending.is_empty() => return Poll::Ready(None),
                None => {
                    return Poll::Ready(Some(Err(tonic::Status::unknown(
                        "truncated gRPC-Web frame",
                    ))));
                }
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.pending.is_empty() && self.inner.is_end_stream()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use http_body_util::{BodyExt, Full, StreamBody};
    use tonic::{Code, Status};
    use tonic_web::GrpcWebClientService;
    use tower::ServiceExt;

    async fn decode<B>(body: B) -> http_body_util::Collected<Bytes>
    where
        B: Body<Data = Bytes> + Unpin,
        B::Error: std::fmt::Display,
    {
        let mut body = Some(body);
        let service = tower::service_fn(
            move |_: Request<tonic_web::GrpcWebCall<tonic::body::Body>>| {
                std::future::ready(Ok::<_, std::convert::Infallible>(Response::new(
                    body.take().unwrap(),
                )))
            },
        );
        GrpcWebClientService::new(service)
            .oneshot(Request::new(tonic::body::Body::empty()))
            .await
            .unwrap()
            .into_body()
            .collect()
            .await
            .unwrap()
    }

    fn response_bytes(status: u8) -> Bytes {
        // A message followed immediately by the final gRPC-Web status frame.
        let mut bytes = vec![0, 0, 0, 0, 3, 1, 2, 3];
        let trailers = format!("grpc-status:{status}\r\n");
        bytes.push(0x80);
        bytes.extend_from_slice(&(trailers.len() as u32).to_be_bytes());
        bytes.extend_from_slice(trailers.as_bytes());
        bytes.into()
    }

    #[tokio::test]
    async fn upstream_decoder_drops_coalesced_trailers_without_workaround() {
        let collected = decode(Full::new(response_bytes(0))).await;
        assert!(collected.trailers().is_none());
    }

    #[tokio::test]
    async fn combined_message_and_trailers_preserve_final_status() {
        for (status, code) in [(0, Code::Ok), (7, Code::PermissionDenied)] {
            let collected = decode(GrpcWebFrames::new(Full::new(response_bytes(status)))).await;
            let trailers = collected
                .trailers()
                .expect("final status must be preserved");
            assert_eq!(Status::from_header_map(trailers).unwrap().code(), code);
            assert_eq!(
                collected.to_bytes(),
                Bytes::from_static(&[0, 0, 0, 0, 3, 1, 2, 3])
            );
        }
    }

    #[tokio::test]
    async fn fragmented_headers_and_payloads_preserve_status() {
        let frames = response_bytes(0)
            .iter()
            .copied()
            .map(|byte| Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(vec![byte]))))
            .collect::<Vec<_>>();
        let body = StreamBody::new(tokio_stream::iter(frames));
        let collected = decode(GrpcWebFrames::new(body)).await;
        assert_eq!(collected.trailers().unwrap()["grpc-status"], "0");
        assert_eq!(
            collected.to_bytes(),
            Bytes::from_static(&[0, 0, 0, 0, 3, 1, 2, 3])
        );
    }

    #[tokio::test]
    async fn missing_status_is_not_replaced_with_success() {
        let body = Full::new(Bytes::from_static(&[0, 0, 0, 0, 3, 1, 2, 3]));
        let collected = decode(GrpcWebFrames::new(body)).await;
        assert!(collected.trailers().is_none());
    }

    #[tokio::test]
    async fn truncated_and_oversized_frames_fail() {
        for bytes in [
            Bytes::from_static(&[0, 0, 0]),
            Bytes::from_static(&[0, 0, 0, 0, 3, 1]),
            Bytes::from_static(&[0x80, 0, 0, 0, 3, 1]),
            Bytes::from_static(&[0, 0, 0x40, 0, 1]),
        ] {
            assert!(
                GrpcWebFrames::new(Full::new(bytes))
                    .collect()
                    .await
                    .is_err()
            );
        }
    }
}
