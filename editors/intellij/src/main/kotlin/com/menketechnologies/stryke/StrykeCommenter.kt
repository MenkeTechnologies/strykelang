package com.menketechnologies.stryke

import com.intellij.lang.Commenter

/**
 * Line comments use `#` (Perl/shell style). Block comments use POD:
 * `=pod ... =cut`, the only multi-line comment form stryke (and Perl)
 * accept. POD markers must start at column 0, which IntelliJ's Cmd+Opt+/
 * shortcut honours by emitting the prefix/suffix on their own lines.
 */
class StrykeCommenter : Commenter {
    override fun getLineCommentPrefix(): String = "# "
    override fun getBlockCommentPrefix(): String = "=pod\n"
    override fun getBlockCommentSuffix(): String = "\n=cut\n"
    override fun getCommentedBlockCommentPrefix(): String = "=pod\n"
    override fun getCommentedBlockCommentSuffix(): String = "\n=cut\n"
}
