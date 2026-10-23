---
title: Basics
author: folio
---

# Getting started

folio renders **Markdown** in the terminal with *italic*, ***bold italic***,
~~strikethrough~~ and `inline code`. Links look like [the docs](https://example.com/docs),
and bare URLs like https://example.com stay readable. Anchors such as
[the install section](#install) don't repeat their target.

## Install

Run the installer, then check the version.[^1] Each step is safe to repeat.[^note]

1. Download the release.
2. Unpack it:
   - on macOS, double-click the archive
   - on Linux, use `tar -xzf folio.tar.gz`
     - nested lists get their own bullet
3. Put `folio` on your `PATH`.

- [x] Write the parser
- [ ] Add images
- [ ] Add diagrams

### Loose lists

- First item with a paragraph.

  A second paragraph in the same item.

- Second item.

> A quote can hold *several* lines of text that wrap
> across the width of the terminal.
>
> > And quotes can nest.

> [!NOTE]
> Alerts get a colored bar and a label.

> [!WARNING]
> This one warns about something.

---

Line one with a hard break  
line two.

![A diagram of the pipeline](pipeline.png)

[![build](https://img.shields.io/badge/build-passing-green)](https://ci.example.com)

<p align="center">
  <img src="logo.png" alt="folio logo">
  <br>
  <b>Read Markdown comfortably</b>
</p>

Term
: The definition of the term.

$$
E = mc^2
$$

Inline math like $a^2 + b^2 = c^2$ is shown as source.

[^1]: The version is printed by `folio --version`.
[^note]: Running it twice changes nothing.
