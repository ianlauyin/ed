# HTML: inline tags

Each line says what you should see.

- Bold: <b>b tag</b> and <strong>strong tag</strong>
- Italic: <i>i tag</i>, <em>em tag</em>, <cite>cite tag</cite>
- Strikethrough: <s>s tag</s>, <del>del tag</del>, <strike>strike tag</strike>
- Underline: <u>u tag</u> and <ins>ins tag</ins>
- Highlighted background: <mark>mark tag</mark>
- Code style: <code>code tag</code>, press <kbd>Ctrl</kbd>+<kbd>C</kbd>, <samp>samp output</samp>
- Superscript/subscript (may render as plain text): x<sup>2</sup>, H<sub>2</sub>O
- Link: <a href="https://example.com">example.com via a tag</a> (clickable)
- Link with title: <a href="https://example.com" title="tooltip">titled link</a>
- Anchor without href renders as text: <a name="here">just text</a>
- Nested: <b>bold <i>bold italic <u>and underlined</u></i></b> then normal
- Unknown tags keep their text: <span style="color:red">span (not red)</span>, <abbr title="x">abbr</abbr>
- Entities: &amp; &lt;tag&gt; &copy; &mdash; &hellip; &#169; &#x2603; &nbsp;(nbsp)
- Comment hidden: before<!-- you should not see this -->after

Line break with br:<br>second line<br/>third line<br />fourth line.

Unclosed inline tag ends with its paragraph: <b>bold until the end of this paragraph

This paragraph must **not** be bold.

Inline image with size: <img src="https://img.shields.io/badge/build-passing-brightgreen" alt="build badge" height="20"> after the badge.

Raw HTML is **only** parsed in the preview. Hover this file's headings in the
editor's hover popovers or notifications to compare: there it shows as raw
text.
