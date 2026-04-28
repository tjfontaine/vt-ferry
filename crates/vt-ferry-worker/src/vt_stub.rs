use crate::backend::Backend;
use vt_ferry_protocol::*;

pub struct VtStubBackend {}

impl VtStubBackend {
    pub fn new() -> Self {
        VtStubBackend {}
    }
}

impl Backend for VtStubBackend {
    fn reset_from_env(&mut self) {}

    fn dispatch(
        &mut self,
        req_header: &MessageHeader,
        _req_payload: &[u8],
        res_header: &mut MessageHeader,
        res_payload: &mut [u8],
    ) -> Result<usize, ()> {
        res_header.version = VTF_TRANSPORT_VERSION;
        res_header.opcode = req_header.opcode;
        res_header.request_id = req_header.request_id;
        res_header.status = STATUS_OK;

        if req_header.version != VTF_TRANSPORT_VERSION {
            res_header.status = STATUS_UNSUPPORTED_VERSION;
            return Ok(0);
        }

        match req_header.opcode {
            OP_HELLO => {
                let payload = HelloReply {
                    worker_abi_version: 1, // VTF_TRANSPORT_VERSION
                    reserved: 0,
                    supported_features: 0,
                    worker_name: *b"vt-ferry-host-worker-vt-stub\0\0\0\0",
                };
                let bytes = bytemuck::bytes_of(&payload);
                res_payload[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            }
            OP_PING => Ok(0),
            OP_GET_CAPS => {
                let payload = GetCapsReply {
                    codec_bits: 0,
                    pixel_format_bits: 0,
                    session_feature_bits: 0,
                    max_width: 0,
                    max_height: 0,
                    max_inflight_frames: 0,
                    reserved: 0,
                };
                let bytes = bytemuck::bytes_of(&payload);
                if bytes.len() > res_payload.len() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                res_payload[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            }
            _ => Err(()),
        }
    }
}
