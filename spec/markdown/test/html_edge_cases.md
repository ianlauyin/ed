# HTML: edge cases

None of these should crash or leave the rest of the document styled wrongly.

## Scripts and styles are dropped

<style>
p { color: red; }
</style>

<script>
alert("<b>should not run or show</b>");
</script>

You should see only this paragraph from this section.

## Stray and mismatched tags

Text with a stray </div> closing tag and </b> stray end.

<div>
Unclosed div at the end of this section.

## Still a normal heading

A lone < sign and a > sign, and a 3 < 4 comparison.

<notatag and an unterminated angle bracket

## Tags across list items

- <b>bold in item one
- item two must not be bold

## HTML inside a blockquote

> <b>bold inside a quote</b>
>
> <div align="center">centered inside a quote</div>

## Code is not HTML

Inline code: `<b>not bold</b>`

```html
<div align="center">
  <b>shown as code, highlighted as HTML</b>
</div>
```

## Very long attribute and odd quoting

<a href='https://example.com/?q="quoted"&amp;x=1' title=unquoted>single-quoted href</a>
<img src=https://img.shields.io/badge/unquoted-src-orange alt=unquoted>

## Empty elements

<p></p>
<div></div>
<b></b>

End of file: the unclosed div above must not affect anything here.
