# Smoltcp for Ixy

Provides IP/ICMP/TCP/UDP/IP for ixy driver devices.

## Implementation

Implements a `smoltcp::phy::Device` from smoltcp on generic instances of an
`ixy::IxyDevice`. This should then be usable to poll into a `SocketSet` and
feed any of the socket implementations provided by `smoltcp`. While the details
of the `smoltcp` dependency are not hidden in an abstraction, it remains to be
seen if the network stack provides the necessary performance characteristics to
compete when built in such a manner.

## Evaluation

A quick and dirty internal evaluation can be performed with loop-back traffic,
establishing one server and one client packet flows respectively [WIP]. 

For a more detailed analysis, a common measuring environment is required.
However, many common tests target the protocol level with reliance on simulator
tools such as [NS-2], for precise control over delay parameters etc., or rely
on the socket interface measuring the kernel implementation (e.g. [Lachlan
et.al][TCPEval]). Both assumptions are not applicable here. Instead, the test
suite needs to abstract over the client machine's network interface and compare
different machines with the same congestion parameters instead of the same
machine with different congestion parameters. The setup complexity also makes
this option badly suited for evaluation during the first implementation phase.
I'm still on the lookout.

[NS-2]: https://www.isi.edu/nsnam/ns/
[TCPEval]: http://users.monash.edu/~lachlana/pubs/TCP-suite-PFLDnet.pdf
