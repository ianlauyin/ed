# HTML: tables and lists

## Table with thead

<table>
  <thead>
    <tr><th>Name</th><th>Type</th><th>Default</th></tr>
  </thead>
  <tbody>
    <tr><td>width</td><td><code>u32</code></td><td>80</td></tr>
    <tr><td>wrap</td><td><code>bool</code></td><td><b>true</b></td></tr>
  </tbody>
</table>

Expect: header row styled like a markdown table header; striped body.

## Table with a leading `<th>` row (no thead)

<table>
<tr><th>Key</th><th>Action</th></tr>
<tr><td><kbd>cmd-s</kbd></td><td>Save</td></tr>
<tr><td><kbd>cmd-p</kbd></td><td>File finder</td></tr>
</table>

## Ragged rows and implicit closing

<table>
<tr><td>a1<td>a2<td>a3
<tr><td>b1
<tr><td>c1<td>c2
</table>

Expect a 3-column grid; short rows padded with empty cells.

## Row headers in the body

<table>
<tr><td></td><th>Mon</th><th>Tue</th></tr>
<tr><th>Morning</th><td>Run</td><td>Swim</td></tr>
<tr><th>Evening</th><td>Read</td><td>Rest</td></tr>
</table>

Expect: row labels bold.

## Caption and colspan (unsupported)

<table>
<caption>This caption is dropped</caption>
<tr><th colspan="2">Spans two columns (renders as one cell)</th></tr>
<tr><td>left</td><td>right</td></tr>
</table>

## Images in a table

<table>
<tr>
<td align="center"><img src="https://img.shields.io/badge/a-1-blue" alt="a"><br>Badge A</td>
<td align="center"><img src="https://img.shields.io/badge/b-2-red" alt="b"><br>Badge B</td>
</tr>
</table>

## Lists

<ul>
  <li>Unordered one</li>
  <li>Unordered two
    <ul>
      <li>Nested</li>
      <li>Nested <b>bold</b></li>
    </ul>
  </li>
</ul>

<ol start="3">
  <li>Starts at three</li>
  <li>Four</li>
</ol>

<ul><li>Implicitly<li>closed<li>items</ul>

## Markdown table next to HTML

| Markdown | Table |
| -------- | ----: |
| for      | 1     |
| comparison | 2   |
