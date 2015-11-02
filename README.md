# Eventual - Futures & Streams for Rust

Eventual provides a Future & Stream abstraction for Rust as well as a
number of computation builders to operate on them.

[![Build Status](https://travis-ci.org/carllerche/eventual.svg?branch=master)](https://travis-ci.org/carllerche/eventual)

- [API documentation](http://carllerche.github.io/eventual/eventual/index.html)

## Usage

To use `Eventual`, first add this to your `Cargo.toml`:

```toml
[dependencies.eventual]
git = "https://github.com/carllerche/eventual"
```

Then, add this to your crate root:

```rust
extern crate eventual;
```

## About-doc-zh-cn

关于中文文档的一些说明：

1. 本文力图还原作者的话，但是涉及翻译能力有限，主业还是程序员，所以有纰漏的地方欢迎大家指正。
2. 关于`译者读代码注`：这个是我[@wayslog](http://github.com/wayslog)的个人读源码的一些解释，涉及到的多半是作者还没有加注释的地方，仅供参考
3. 欢迎和我一起维护中文文档
