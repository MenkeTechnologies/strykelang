" Vim configuration for perlrs
" Add to your .vimrc or source this file
"
" Usage:
"   source /path/to/perlrs/editors/perlrs.vim

" Register .pr files as perlrs filetype with perl syntax
augroup perlrs_filetype
  autocmd!
  autocmd BufNewFile,BufRead *.pr setfiletype perlrs
  autocmd FileType perlrs setlocal syntax=perl
  autocmd FileType perlrs setlocal commentstring=#\ %s
augroup END

" For vim-lsp plugin (https://github.com/prabirshrestha/vim-lsp)
if exists('*lsp#register_server')
  call lsp#register_server({
    \ 'name': 'perlrs',
    \ 'cmd': ['pe', '--lsp'],
    \ 'allowlist': ['perlrs', 'perl'],
    \ })
endif

" For coc.nvim, add to coc-settings.json:
" {
"   "languageserver": {
"     "perlrs": {
"       "command": "pe",
"       "args": ["--lsp"],
"       "filetypes": ["perlrs", "perl"]
"     }
"   }
" }
