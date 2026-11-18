# install lsp manually, or it will be run by npx/uvx

```sh
# json, css, html
npm install -g vscode-langservers-extracted
# typescript / javascript: TypeScript 7 (`tsc --lsp`), from the project's node_modules
npm install -D typescript@^7
# formatting (js/ts/json/css), picked by the project's biome.json or prettier config
npm install -D @biomejs/biome

# terraform
sudo apt install terraform-ls
```
