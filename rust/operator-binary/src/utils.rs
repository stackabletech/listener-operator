use std::{os::unix::prelude::AsRawFd, path::Path};

use pin_project::pin_project;
use socket2::Socket;
use stackable_operator::{commons::listener::AddressType, k8s_openapi::api::core::v1::Node};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{UnixListener, UnixStream},
};
use tonic::transport::server::Connected;

/// Adapter for using [`UnixStream`] as a [`tonic`] connection
/// Tonic usually communicates via TCP sockets, but the Kubernetes CSI interface expects
/// plugins to use Unix sockets instead.
/// This provides a wrapper implementation which delegates to tokio's [`UnixStream`] in order
/// to enable tonic to communicate via Unix sockets.
#[pin_project]
pub struct TonicUnixStream(#[pin] pub UnixStream);

impl AsyncRead for TonicUnixStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.project().0.poll_read(cx, buf)
    }
}

impl AsyncWrite for TonicUnixStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        self.project().0.poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.project().0.poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.project().0.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        self.project().0.poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }
}

impl Connected for TonicUnixStream {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

/// Bind a Unix Domain Socket listener that is only accessible to the current user
pub fn uds_bind_private(path: impl AsRef<Path>) -> Result<UnixListener, std::io::Error> {
    // Workaround for https://github.com/tokio-rs/tokio/issues/4422
    let socket = Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    unsafe {
        // Socket-level chmod is propagated to the file created by Socket::bind.
        // We need to chmod /before/ creating the file, because otherwise there is a brief window where
        // the file is world-accessible (unless restricted by the global umask).
        if libc::fchmod(socket.as_raw_fd(), 0o600) == -1 {
            return Err(std::io::Error::last_os_error());
        }
    }
    socket.bind(&socket2::SockAddr::unix(path)?)?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    UnixListener::from_std(socket.into())
}

/// Combines the messages of an error and its sources into a [`String`] of the form `"error: source 1: source 2: root error"`
pub fn error_full_message(err: &dyn std::error::Error) -> String {
    use std::fmt::Write;
    // Build the full hierarchy of error messages by walking up the stack until an error
    // without `source` set is encountered and concatenating all encountered error strings.
    let mut full_msg = format!("{}", err);
    let mut curr_err = err.source();
    while let Some(curr_source) = curr_err {
        write!(full_msg, ": {curr_source}").expect("string formatting should be infallible");
        curr_err = curr_source.source();
    }
    full_msg
}

#[derive(Debug, Clone, Copy)]
pub struct AddressCandidates<'a> {
    pub ip: Option<&'a str>,
    pub hostname: Option<&'a str>,
}

impl<'a> AddressCandidates<'a> {
    pub fn pick(&self, preferred_address_type: AddressType) -> Option<(&'a str, AddressType)> {
        let ip = self.ip.zip(Some(AddressType::Ip));
        let hostname = self.hostname.zip(Some(AddressType::Hostname));
        match preferred_address_type {
            AddressType::Ip => ip.or(hostname),
            AddressType::Hostname => hostname.or(ip),
        }
    }
}

/// Try to guess the primary address of a Node, which it is expected that external clients should be able to reach it on
pub fn node_primary_address(node: &Node) -> AddressCandidates {
    let addrs = node
        .status
        .as_ref()
        .and_then(|s| s.addresses.as_deref())
        .unwrap_or_default();

    AddressCandidates {
        ip: addrs
            .iter()
            .find(|addr| addr.type_ == "ExternalIP")
            .or_else(|| addrs.iter().find(|addr| addr.type_ == "InternalIP"))
            .map(|addr| addr.address.as_str()),
        hostname: addrs
            .iter()
            .find(|addr| addr.type_ == "Hostname")
            .map(|addr| addr.address.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::error_full_message;

    #[test]
    fn error_messages() {
        assert_eq!(
            error_full_message(anyhow::anyhow!("standalone error").as_ref()),
            "standalone error"
        );
        assert_eq!(
            error_full_message(
                anyhow::anyhow!("root error")
                    .context("middleware")
                    .context("leaf")
                    .as_ref()
            ),
            "leaf: middleware: root error"
        );
    }
}
