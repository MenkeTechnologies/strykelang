-- Neovim LSP configuration for stryke
-- Add to your init.lua or source this file
--
-- Usage:
--   require('stryke') -- if in lua path
--   -- or --
--   dofile('/path/to/stryke/editors/stryke.lua')

-- Register .stk files as stryke filetype
vim.filetype.add({
  extension = {
    ['stk'] = 'stryke',
  },
})

-- Use perl syntax highlighting for stryke files
vim.api.nvim_create_autocmd('FileType', {
  pattern = 'stryke',
  callback = function()
    vim.bo.syntax = 'perl'
    vim.bo.commentstring = '# %s'
  end,
})

-- LSP configuration using nvim-lspconfig
local ok, lspconfig = pcall(require, 'lspconfig')
if ok then
  local configs = require('lspconfig.configs')

  if not configs.stryke then
    configs.stryke = {
      default_config = {
        cmd = { 'st', '--lsp' },
        filetypes = { 'stryke', 'perl' },
        root_dir = function(fname)
          return lspconfig.util.root_pattern('.git', 'Makefile.PL', 'cpanfile', 'dist.ini')(fname)
            or lspconfig.util.path.dirname(fname)
        end,
        single_file_support = true,
        settings = {},
      },
    }
  end

  lspconfig.stryke.setup({
    on_attach = function(client, bufnr)
      local opts = { buffer = bufnr, noremap = true, silent = true }
      vim.keymap.set('n', 'gd', vim.lsp.buf.definition, opts)
      vim.keymap.set('n', 'gD', vim.lsp.buf.declaration, opts)
      vim.keymap.set('n', 'K', vim.lsp.buf.hover, opts)
      vim.keymap.set('n', 'gr', vim.lsp.buf.references, opts)
      vim.keymap.set('n', '<leader>rn', vim.lsp.buf.rename, opts)
      vim.keymap.set('n', '<leader>ca', vim.lsp.buf.code_action, opts)
      vim.keymap.set('i', '<C-k>', vim.lsp.buf.signature_help, opts)
    end,
    capabilities = vim.lsp.protocol.make_client_capabilities(),
  })
else
  -- Fallback: manual LSP setup without lspconfig
  vim.api.nvim_create_autocmd('FileType', {
    pattern = { 'stryke', 'perl' },
    callback = function()
      vim.lsp.start({
        name = 'stryke',
        cmd = { 'st', '--lsp' },
        root_dir = vim.fn.getcwd(),
      })
    end,
  })
end
