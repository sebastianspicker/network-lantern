//! Keeps the Windows qWAVE flow alive for the complete data-stream lifetime.

#[cfg(windows)]
use std::os::windows::io::{AsRawSocket, AsSocket};

/// Owns the qWAVE handle associated with one connected socket.
///
/// On non-Windows systems traffic class is applied with the socket API and the
/// empty guard keeps the stream representation platform-independent.
pub(super) struct TrafficClassFlow {
    #[cfg(windows)]
    qos_handle: usize,
    #[cfg(windows)]
    flow_id: u32,
}

#[cfg(any(windows, test))]
fn dscp_from_tos(tos: u8) -> std::io::Result<u32> {
    if tos == 0 || tos & 0b11 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "traffic class must contain a nonzero six-bit DSCP value with ECN bits clear",
        ));
    }
    Ok(u32::from(tos >> 2))
}

#[cfg(windows)]
pub(super) fn apply<S: AsSocket>(socket: &S, tos: u8) -> std::io::Result<TrafficClassFlow> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, HANDLE},
        NetworkManagement::QoS::{
            QOS_NON_ADAPTIVE_FLOW, QOS_VERSION, QOSAddSocketToFlow, QOSCloseHandle,
            QOSCreateHandle, QOSRemoveSocketFromFlow, QOSSetFlow, QOSSetOutgoingDSCPValue,
            QOSTrafficTypeBestEffort,
        },
    };

    let dscp = dscp_from_tos(tos)?;
    let version = QOS_VERSION {
        MajorVersion: 1,
        MinorVersion: 0,
    };
    let mut qos_handle: HANDLE = std::ptr::null_mut();
    if unsafe { QOSCreateHandle(&version, &mut qos_handle) } == 0 {
        return Err(std::io::Error::from_raw_os_error(unsafe {
            GetLastError() as i32
        }));
    }
    let socket = socket.as_socket().as_raw_socket() as usize;
    let mut flow_id = 0_u32;
    if unsafe {
        QOSAddSocketToFlow(
            qos_handle,
            socket,
            std::ptr::null(),
            QOSTrafficTypeBestEffort,
            QOS_NON_ADAPTIVE_FLOW,
            &mut flow_id,
        )
    } == 0
    {
        let error = std::io::Error::from_raw_os_error(unsafe { GetLastError() as i32 });
        unsafe { QOSCloseHandle(qos_handle) };
        return Err(error);
    }
    if unsafe {
        QOSSetFlow(
            qos_handle,
            flow_id,
            QOSSetOutgoingDSCPValue,
            std::mem::size_of::<u32>() as u32,
            (&raw const dscp).cast(),
            0,
            std::ptr::null_mut(),
        )
    } == 0
    {
        let error = std::io::Error::from_raw_os_error(unsafe { GetLastError() as i32 });
        unsafe {
            QOSRemoveSocketFromFlow(qos_handle, 0, flow_id, 0);
            QOSCloseHandle(qos_handle);
        }
        return Err(error);
    }
    Ok(TrafficClassFlow {
        qos_handle: qos_handle as usize,
        flow_id,
    })
}

#[cfg(windows)]
impl Drop for TrafficClassFlow {
    fn drop(&mut self) {
        use windows_sys::Win32::NetworkManagement::QoS::{QOSCloseHandle, QOSRemoveSocketFromFlow};
        let handle = self.qos_handle as windows_sys::Win32::Foundation::HANDLE;
        unsafe {
            // A zero socket destroys the complete single-socket flow before
            // closing the subsystem handle.
            QOSRemoveSocketFromFlow(handle, 0, self.flow_id, 0);
            QOSCloseHandle(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dscp_from_tos;

    #[test]
    fn converts_dscp_tos_and_rejects_ecn_or_best_effort() {
        assert_eq!(dscp_from_tos(184).unwrap(), 46);
        assert_eq!(dscp_from_tos(40).unwrap(), 10);
        assert!(dscp_from_tos(0).is_err());
        assert!(dscp_from_tos(185).is_err());
    }
}
