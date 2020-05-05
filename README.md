# `async-on-embedded`

> `async fn` / `.await` on embedded (Cortex-M edition): no-global-alloc, no-threads runtime

**Status:** Proof of Concept. Do not use in production.

Thanks for checking out our work! We wrote this PoC to test out the [compiler work] we did to enable the use of async/await in `no_std` code.

[compiler work]: https://github.com/rust-lang/rust/pull/69033

We don't intend to continue to work on this repository, or accept pull requests, for the time being but you are more than welcome to use it as a reference! The code is permissively licensed (MIT / Apache-2).

If you are interested in using the async/await in your embedded project or evaluate it for a project, [give us a call]! We do consulting.

[give us a call]: https://ferrous-systems.com/#contact

To learn more about embedded async/await work check out this series of blog posts we wrote on the topic:

- [Bringing async/await to embedded Rust](https://ferrous-systems.com/blog/embedded-async-await/) 
- [async/await on embedded Rust](https://ferrous-systems.com/blog/async-on-embedded/)
- [no_std async/await - soon on stable](https://ferrous-systems.com/blog/stable-async-on-embedded/)

---

Check `nrf52/examples` for an overview of what can be done with the runtime.

**NOTE** You need a rustc build that includes PR [rust-lang/rust#69033]

[rust-lang/rust#69033]: https://github.com/rust-lang/rust/pull/69033

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
licensed as above, without any additional terms or conditions.
