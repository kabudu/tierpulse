# Contributing to Tierpulse

First off, thank you for considering contributing to Tierpulse! It's people like you that make Tierpulse a great tool.

## Code of Conduct

By participating in this project, you agree to abide by the [Contributor Covenant Code of Conduct](https://www.contributor-covenant.org/version/2/1/code_of_conduct/code_of_conduct.md).

## How Can I Contribute?

### Reporting Bugs

- **Check the FAQ** to see if your issue has already been addressed.
- **Search existing issues** to see if the bug has already been reported.
- **Submit a new issue** if you can't find an existing one. Include a clear title, description, and steps to reproduce.

### Suggesting Enhancements

- **Check existing issues** to see if the enhancement has already been suggested.
- **Submit a new issue** describing the enhancement and why it would be useful.

### Pull Requests

1. **Fork the repository** and create your branch from `master`.
2. **If you've added code that should be tested, add tests.**
3. **Ensure the test suite passes** by running `cargo test`.
4. **Follow the Rust style guide** and run `cargo fmt` before submitting.
5. **Update documentation** as necessary.
6. **Open a pull request** with a clear title and description of your changes.

## Development Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Docker](https://docs.docker.com/get-docker/) (for containerized builds)
- [Python 3.11+](https://www.python.org/downloads/) (for model export tasks)

### Running Tests

```bash
cargo test
```

### Checking Style

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Pull Request Process

1. Ensure any install or build dependencies are removed before the end of the layer when doing a build.
2. Update the README.md with details of changes to the interface, this includes new environment variables, exposed ports, and useful file locations.
3. You may merge the Pull Request once you have the sign-off of two other developers, or if you do not have permission to do so, a maintainer will merge it for you.

## Recognition

We value all contributions, from documentation to core features. Your name will be added to our contributors list!
