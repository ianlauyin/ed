# HTML: block elements

## Centered README header

<div align="center">
  <img src="https://raw.githubusercontent.com/github/explore/main/topics/rust/rust.png" width="96" alt="Rust logo">
  <h1>Centered title</h1>
  <p>
    <a href="https://example.com"><img src="https://img.shields.io/badge/license-MIT-blue" alt="license"></a>
    <a href="https://example.com"><img src="https://img.shields.io/badge/version-1.0-green" alt="version"></a>
  </p>
  <p>Centered paragraph text.</p>
</div>

Expect: logo, title, two clickable badges and the paragraph, all centered.

## Alignment

<p align="left">Left-aligned paragraph.</p>
<p align="center">Centered paragraph.</p>
<p align="right">Right-aligned paragraph.</p>
<h3 align="right">Right-aligned heading</h3>
<center>Center tag content</center>

## HTML wrapping markdown

<div align="center">

### Markdown heading inside a centered div

Markdown **paragraph** with a [link](https://example.com), centered.

</div>

This paragraph is back to normal alignment.

## Details / summary

<details>
<summary>Click to expand (always expanded here)</summary>

Hidden content with **markdown**:

- item one
- item two

</details>

<details><summary>Inline details</summary>Body text on the same block.</details>

## Paragraphs and headings

<h1>HTML h1</h1>
<h2>HTML h2</h2>
<h3>HTML h3</h3>
<h4>HTML h4</h4>
<h5>HTML h5</h5>
<h6>HTML h6</h6>

<p>First HTML paragraph.
Wrapped source line that should join the same paragraph.</p>
<p>Second HTML paragraph.

<p>Unclosed paragraph closed by the next block.
<h3>Heading after unclosed p</h3>

## Blockquote and rule

<blockquote>
  <p>HTML blockquote with a <b>bold</b> word.</p>
</blockquote>

Above a rule:
<hr>
Below the rule.

## Preformatted

<pre>
fn preformatted() {
    // indentation is kept
    let x = 1 &lt; 2;
}
</pre>

Expect a code block with the indentation kept and `<` decoded.
