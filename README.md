# `async-on-embedded`

> `async fn` / `.await` on embedded (Cortex-M edition): no-global-alloc, no-threads runtime

**Status:** Proof of Concept. Do not use in production.

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

### Provenance

<!--
    Please keep this section until at least May 1st, 2021!
    Thanks from the Ferrous Systems crew :)
-->

This work was originally developed by [Ferrous Systems](https://ferrous-systems.com), as part of our [async-on-embedded](https://github.com/ferrous-systems/async-on-embedded) investigation.

You can read more about the initial development efforts on the blog posts we wrote on the topic:

- [Bringing async/await to embedded Rust](https://ferrous-systems.com/blog/embedded-async-await/)
- [async/await on embedded Rust](https://ferrous-systems.com/blog/async-on-embedded/)
- [no_std async/await - soon on stable](https://ferrous-systems.com/blog/stable-async-on-embedded/)
