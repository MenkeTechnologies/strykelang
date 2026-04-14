-- Neovim LSP configuration for perlrs
-- Add to your init.lua or source this file
--
-- Usage:
--   require('perlrs') -- if in lua path
--   -- or --
--   dofile('/path/to/perlrs/editors/perlrs.lua')

-- Register .pr files as perlrs filetype
vim.filetype.add({
  extension = {
    pr = 'perlrs',
  },
})

-- Use perl syntax highlighting for perlrs files
vim.api.nvim_create_autocmd('FileType', {
  pattern = 'perlrs',
  callback = function()
    vim.bo.syntax = 'perl'
    vim.bo.commentstring = '# %s'
  end,
})

-- LSP configuration using nvim-lspconfig
local ok, lspconfig = pcall(require, 'lspconfig')
if ok then
  local configs = require('lspconfig.configs')

  if not configs.perlrs then
    configs.perlrs = {
      default_config = {
        cmd = { 'pe', '--lsp' },
        filetypes = { 'perlrs', 'perl' },
        root_dir = function(fname)
          return lspconfig.util.root_pattern('.git', 'Makefile.PL', 'cpanfile', 'dist.ini')(fname)
            or lspconfig.util.path.dirname(fname)
        end,
        single_file_support = true,
        settings = {},
      },
    }
  end

  lspconfig.perlrs.setup({
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
    pattern = { 'perlrs', 'perl' },
    callback = function()
      vim.lsp.start({
        name = 'perlrs',
        cmd = { 'pe', '--lsp' },
        root_dir = vim.fn.getcwd(),
      })
    end,
  })
end
