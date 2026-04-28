use vt_ferry_protocol::*;

/// One backend instance per accepted connection. The server spawns
/// a thread per connection (so two guest processes can transcode
/// concurrently through the same broker), and each thread owns its
/// own `Box<dyn Backend>` produced by a `BackendFactory`. The
/// `Send + 'static` bound is what lets `Box<dyn Backend>` move
/// into a freshly spawned thread.
///
/// Backends MUST be safe to use from a single thread at a time;
/// they don't need to be `Sync`. Concurrent connections each have
/// their own backend instance — there's no cross-connection
/// session sharing.
pub trait Backend: Send + 'static {
    fn reset_from_env(&mut self);
    fn dispatch(
        &mut self,
        req_header: &MessageHeader,
        req_payload: &[u8],
        res_header: &mut MessageHeader,
        res_payload: &mut [u8],
    ) -> Result<usize, ()>; // Returns length of the response payload written
}

/// Produces fresh `Backend` instances on demand. The server holds
/// one factory shared across all accepted connections; per-accept
/// it calls `factory.create()` to get a new backend for the
/// connection's thread.
///
/// Factories carry the host-process-wide shared state — most
/// notably the `IOSurfacePoolDirectory` for the vt-real backend,
/// which holds launcher-registered IOSurfaces that have to be
/// shared across all connections (each `take_matching` consumes
/// one entry, so two concurrent guests competing for the same
/// shape will see one succeed and one fall back).
pub trait BackendFactory: Send + Sync + 'static {
    fn create(&self) -> Box<dyn Backend>;
}
