package com.menketechnologies.stryke

import com.intellij.lang.Commenter

/**
 * Line comments use `#` (Perl/shell style). Block-comment prefix/suffix
 * are intentionally `null` because stryke's only block-comment form is
 * POD (`=pod ... =cut`) which requires the markers to start at column 0
 * — IntelliJ's default `CommentByBlockCommentHandler` inserts the
 * prefix/suffix at the selection anchors without re-anchoring to BOL,
 * which produces invalid POD (e.g. `foo(=pod\n...\n=cut\n)`) and was
 * also breaking the line-comment action under some platform versions.
 * Users can still comment a whole selection by line-commenting (Cmd-/)
 * with multi-line selection — IntelliJ prepends `# ` to every line.
 */
class StrykeCommenter : Commenter {
    override fun getLineCommentPrefix(): String = "# "
    override fun getBlockCommentPrefix(): String? = null
    override fun getBlockCommentSuffix(): String? = null
    override fun getCommentedBlockCommentPrefix(): String? = null
    override fun getCommentedBlockCommentSuffix(): String? = null
}
