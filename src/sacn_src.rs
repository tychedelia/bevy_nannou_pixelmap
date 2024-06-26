use bevy::prelude::*;
use sacn::packet::ACN_SDT_MULTICAST_PORT;
use std::net::{IpAddr, SocketAddr, UdpSocket};

#[derive(Resource, Deref, DerefMut)]
pub struct SacnSource(sacn::source::SacnSource);

impl SacnSource {
    pub fn new(interface_ip: IpAddr) -> Self {
        let src = sacn::source::SacnSource::with_ip(
            "nannou",
            SocketAddr::new(interface_ip, ACN_SDT_MULTICAST_PORT),
        )
        .unwrap();
        Self(src)
    }
}
