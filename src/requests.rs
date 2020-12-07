//! Request generation based on the user’s input arguments.

use dns::Labels;
use dns_transport::Error as TransportError;

use crate::connect::TransportType;
use crate::resolve::Resolver;
use crate::txid::TxidGenerator;


/// All the information necessary to generate requests for one or more
/// queries, nameservers, or transport types.
#[derive(PartialEq, Debug)]
pub struct RequestGenerator {

    /// The input parameter matrix.
    pub inputs: Inputs,

    /// How to generate transaction IDs.
    pub txid_generator: TxidGenerator,

    /// Whether to OPT in to DNS extensions.
    pub edns: UseEDNS,

    /// Other weird protocol options.
    pub protocol_tweaks: ProtocolTweaks,
}

/// Which things the user has specified they want queried.
#[derive(PartialEq, Debug, Default)]
pub struct Inputs {

    /// The list of domain names to query.
    pub domains: Vec<Labels>,

    /// The list of DNS record types to query for.
    pub types: Vec<u16>,

    /// The list of DNS classes to query for.
    pub classes: Vec<dns::QClass>,

    /// The list of resolvers to send queries to.
    pub resolvers: Vec<Resolver>,

    /// The list of transport types to send queries over.
    pub transport_types: Vec<TransportType>,
}

/// Weird protocol options that are allowed by the spec but are not common.
#[derive(PartialEq, Debug, Default, Copy, Clone)]
pub struct ProtocolTweaks {

    /// Set the `AA` (Authoritative Answer) flag in the header of each request.
    pub set_authoritative_flag: bool,

    /// Set the `AD` (Authentic Data) flag in the header of each request.
    pub set_authentic_flag: bool,

    /// Set the `CD` (Checking Disabled) flag in the header of each request.
    pub set_checking_disabled_flag: bool,

    /// Set the buffer size field in the OPT record of each request.
    pub udp_payload_size: Option<u16>,
}

/// Whether to send or display OPT packets.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum UseEDNS {

    /// Do not send an OPT query in requests, and do not display them.
    Disable,

    /// Send an OPT query in requests, but hide the result. This is the
    /// default, because the information is usually not useful to the user.
    SendAndHide,

    /// Send an OPT query in requests, _and_ display any OPT records in the
    /// response we receive.
    SendAndShow,
}


impl RequestGenerator {

    /// Iterate through the inputs matrix, returning pairs of DNS requests and
    /// the details of the transport to send them down.
    pub fn generate(self) -> Result<Vec<(dns::Request, Box<dyn dns_transport::Transport>)>, TransportError> {
        let nameservers = self.inputs.resolvers.into_iter()
                              .filter_map(|e| match e.lookup() {
                                  Ok(Some(r)) => Some(r),
                                  _ => None,
                              })
                              .collect::<Vec<_>>();

        if nameservers.is_empty() {
            return Err(TransportError::NoNameservers);
        }

        let mut requests = Vec::new();
        for domain in &self.inputs.domains {
            for qtype in self.inputs.types.iter().copied() {
                for qclass in self.inputs.classes.iter().copied() {
                    for nameserver in &nameservers {
                        for transport_type in &self.inputs.transport_types {

                            let transaction_id = self.txid_generator.generate();
                            let mut flags = dns::Flags::query();
                            self.protocol_tweaks.set_request_flags(&mut flags);

                            let mut additional = None;
                            if self.edns.should_send() {
                                let mut opt = dns::Request::additional_record();
                                self.protocol_tweaks.set_request_opt_fields(&mut opt);
                                additional = Some(opt);
                            }

                            let query = dns::Query { qname: domain.clone(), qtype, qclass };
                            let request = dns::Request { transaction_id, flags, query, additional };

                            let transport = transport_type.make_transport(nameserver.clone());
                            requests.push((request, transport));
                        }
                    }
                }
            }
        }

        Ok(requests)
    }
}

impl UseEDNS {

    /// Whether the user wants to send OPT records.
    pub fn should_send(self) -> bool {
        self != Self::Disable
    }

    /// Whether the user wants to display sent OPT records.
    pub fn should_show(self) -> bool {
        self == Self::SendAndShow
    }
}

impl ProtocolTweaks {
    pub fn set_request_flags(self, flags: &mut dns::Flags) {
        if self.set_authoritative_flag {
            flags.authoritative = true;
        }

        if self.set_authentic_flag {
            flags.authentic_data = true;
        }

        if self.set_checking_disabled_flag {
            flags.checking_disabled = true;
        }
    }

    pub fn set_request_opt_fields(self, opt: &mut dns::record::OPT) {
        if let Some(bufsize) = self.udp_payload_size {
            opt.udp_payload_size = bufsize;
        }
    }
}
