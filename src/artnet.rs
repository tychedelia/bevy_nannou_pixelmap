use artnet_protocol::{ArtCommand, Output, Poll};
use bevy::prelude::*;
use std::net::{SocketAddr, UdpSocket};
use std::thread;

#[derive(Resource)]
pub struct ArtNetServer {
    tx_send: crossbeam_channel::Sender<ArtCommand>,
    rx_recv: crossbeam_channel::Receiver<ArtCommand>,
}

impl ArtNetServer {
    pub fn new() -> Self {
        let (tx_send, rx_send) = crossbeam_channel::unbounded::<ArtCommand>();
        let (tx_recv, rx_recv) = crossbeam_channel::unbounded::<ArtCommand>();

        let socket = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )
        .unwrap();
        socket.set_reuse_address(true).unwrap();
        socket.set_reuse_port(true).unwrap();
        socket.set_broadcast(true).unwrap();
        let broadcast_addr: SocketAddr = "0.0.0.0:6454".parse().unwrap();
        socket.bind(&broadcast_addr.into()).unwrap();
        let socket = UdpSocket::from(socket);
        let output_socket = socket.try_clone().expect("Failed to clone socket");
        let input_socket = socket.try_clone().expect("Failed to clone socket");

        thread::spawn(move || loop {
            let mut buffer = [0u8; 1024];
            let (length, addr) = input_socket.recv_from(&mut buffer).unwrap();
            let command = ArtCommand::from_buffer(&buffer[..length]).unwrap();
            tx_recv.send(command).unwrap();
        });

        thread::spawn(move || loop {
            if let Ok(command) = rx_send.recv() {
                let buff = command
                    .write_to_buffer()
                    .expect("Failed to write command to buffer");
                match output_socket.send_to(&buff, &broadcast_addr) {
                    Ok(_) => {}
                    Err(e) => eprintln!("Failed to send data: {}", e),
                }
            }
        });

        Self { tx_send, rx_recv }
    }

    pub fn send(&self, command: ArtCommand) {
        self.tx_send.send(command).unwrap();
    }

    pub fn poll(&self) -> Option<ArtCommand> {
        self.rx_recv.try_recv().ok()
    }

    pub fn poll_all(&self) -> Vec<ArtCommand> {
        let mut commands = Vec::new();
        while let Ok(command) = self.rx_recv.try_recv() {
            commands.push(command);
        }
        commands
    }
}

pub(crate) struct ArtNetPlugin;

impl Plugin for ArtNetPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ArtNetServer::new());
    }
}
