# Linux enforcement validation

Run privileged checks only in a disposable Linux VM or test host. Do not apply test policies to SSH, the controlling terminal, or production services.

## Build and selection

1. Record `uname -a`, Rust versions, cgroup mounts, and enabled Cargo features.
2. Build from a clean checkout using `cargo xtask build-release --features linux-full`.
3. Confirm all three BPF objects exist in `target/bpfel-unknown-none/release` and no missing-program warning is emitted by the application build.
4. Confirm the running application reports eBPF as available and selected. A successful userspace build alone is insufficient.

## Enforcement matrix

Use a disposable sender/receiver pair and measure baseline throughput before applying limits. For each case, record the requested backend, actual backend, requested rate, achieved rate, burst behavior, and application errors.

| Dimension            | Cases                                                    |
| -------------------- | -------------------------------------------------------- |
| Protocol             | IPv4, IPv6                                               |
| Direction            | upload, download                                         |
| Remote address class | public, private/ULA, link-local, loopback                |
| Policy               | all, internet only, local only                           |
| Transport            | TCP, UDP                                                 |
| Process lifecycle    | existing socket, new socket, child process, process exit |

Use public-address test routes inside the isolated environment rather than generating load against an unrelated internet host. Verify that traffic outside the selected class remains unaffected. Ingress uses the remote source; egress uses the remote destination. Packet offsets are relative to the IP header at the cgroup-SKB hook.

## Failure and cleanup

- Exercise unavailable BPF support, missing permissions, and missing IFB; record errors/fallback rather than assuming equivalence.
- Remove policies and confirm baseline throughput returns.
- Stop the application normally and inspect cgroup membership, attached programs, qdiscs, and nftables state.
- Repeat after abnormal termination and document any manual cleanup needed.
- Check two processes independently, including processes sharing a parent cgroup.

The host classifier regression tests cover address selection and malformed packets. They do not prove verifier compatibility, rate accuracy, concurrency behavior of BPF map updates, or cleanup. Until this matrix is run on the target kernel, describe enforcement as experimental.
