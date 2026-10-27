# Spell checking

## Behaviour

- Built-in, in-process English (en_US) spell checker; no language server,
  no network. The Hunspell dictionary (SCOWL en_US-large) and a list of
  programming terms (from codebook) are bundled in the `spell_check` crate,
  checked with `spellbook`.
- What is checked comes from each grammar's `spellcheck.scm` query:
  `@spell` marks text to check, `@nospell` carves text out, across syntax
  layers. Every bundled grammar has one; a new grammar needs one too.
  - Code (Rust, Go, Python, TypeScript/TSX/JavaScript, Bash): comments,
    string contents, and names where they are defined (functions,
    parameters, variables, types, fields, ...). Uses of names, trait impl
    methods, and module specifiers (imports, `require`) are not checked.
  - Markup/data: Markdown paragraph, heading and table text (code spans,
    link targets, autolinks and HTML tags skipped; fenced code is checked
    by its own language's query); HTML text, comments and `alt`/`title`/
    `placeholder`/`label`/`aria-*` values; CSS comments and class/id names;
    JSON/JSONC/YAML values and comments, not keys (schema-defined);
    `.env` comments only.
  - Git Commit (no grammar): every line except `#` comments and the diff
    below the scissors line. Plain Text: only `.txt` files and untitled
    buffers, since languages without support also open as Plain Text.
- Identifiers are split into camelCase/snake_case words; words under 3
  letters, words with digits, non-Latin scripts, URLs, emails, paths and
  shebangs, hex hashes, UUIDs, base64 and backtick code are skipped.
- Only the visible rows are checked, in the background, after edits,
  reparses and scrolls (debounced). Misspellings are wavy underlines in the
  theme's info color; they are not diagnostics and do not appear in the
  diagnostics panel. Full and auto-height editors only.
- Words are only allowed per project: `spellcheck.txt` at a worktree root
  (one word per line, `#` comments, case-insensitive), used only when it
  exists. There is no global word list. It is read through the project's
  buffer, so unsaved edits apply immediately and remote projects work.
- On a misspelled word, code actions offer up to 5 suggestions and
  "Add … to spellcheck.txt", which inserts the word in case-insensitive
  sorted position (comments stay put), skips words already listed, and
  saves the file (creating it if needed).

## Dictionaries

- `crates/spell_check/dictionaries/`: `en_US.aff` / `en_US.dic` (Hunspell)
  and `code.dic` (programming terms).
- Code words are added to the en_US dictionary at load, not kept in a
  separate list, so they get Hunspell's case rules, affix rules and
  suggestions (typos can be corrected to a code word).
- `code.dic` is one `.dic` entry per line (no count header): `word` or `word/FLAGS`,
  using the affix flags of `en_US.aff`. A plain word accepts only itself;
  add flags by hand where other forms are wanted (`addr/S`, `mmap/SDG`).
  An invalid entry panics on load (covered by the crate's test).
- Entries are lowercase: a lowercase entry also matches Title and UPPER
  case. A mixed-case entry matches only that case (and UPPER).
- Project words (`spellcheck.txt`) are not added to the shared dictionary;
  they stay exact, case-insensitive matches with no affixes.

### Affix flags (en_US.aff)

Hunspell picks the right spelling per word ending (`proxy/S` gives
"proxies", `make/G` gives "making"). Flags combine: `/SDG`.

| Flag | Adds | Example |
| --- | --- | --- |
| `S` | plural / 3rd person: -s, -es, -ies | `addr/S` → addrs, `regex/S` → regexes |
| `M` | possessive 's | `addr/M` → addr's |
| `D` | past tense: -ed, -d, -ied | `mmap/D` → mmaped |
| `G` | -ing | `mmap/G` → mmaping |
| `J` | -ings | `build/J` → buildings |
| `R` | -er, -r, -ier | `lint/R` → linter |
| `Z` | -ers, -rs, -iers | `lint/Z` → linters |
| `T` | -est, -st, -iest | |
| `Y` | -ly | |
| `P` | -ness, -iness | |
| `B` | -able | `parse/B` → parsable |
| `L` | -ment | |
| `N` | -ion, -ication, -en | |
| `X` | -ions, -ications, -ens | |
| `V` | -ive | |
| `H` | -th, -ieth | |
| `A` | re- prefix | `index/A` → reindex |
| `I` | in- prefix | |
| `U` | un- prefix | `map/U` → unmap |
| `C` | de- prefix | `serialize/C` → deserialize |
| `E` | dis- prefix | |
| `F` | con- prefix | |
| `K` | pro- prefix | |

- A prefix and a suffix combine when both are cross-product (`Y` in the
  `.aff`; all but `V`, `H`, `T`): `map/US` → unmaps.
- No consonant doubling: `mmap/D` gives "mmaped", not "mmapped"; list such
  forms as their own entries.
