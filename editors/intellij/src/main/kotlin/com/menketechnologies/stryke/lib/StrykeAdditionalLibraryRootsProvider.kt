package com.menketechnologies.stryke.lib

import com.intellij.icons.AllIcons
import com.intellij.navigation.ItemPresentation
import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.AdditionalLibraryRootsProvider
import com.intellij.openapi.roots.SyntheticLibrary
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.menketechnologies.stryke.StrykeIcons
import java.io.File
import javax.swing.Icon

/// Exposes `~/.stryke/store/<pkg>@<ver>/lib/` directories as
/// SyntheticLibraries so the IDE indexes them and the contents show up
/// under "External Libraries" in the Project view. The static analyzer
/// (LSP server) already chases `use Foo::Bar` into the store via
/// `resolve_require_path_from_file`; this provider is the IDE-side
/// counterpart so PSI / file index / goto-declaration all see the
/// store source as part of the project search scope.
///
/// Without this provider, Cmd-B on a package reference whose decl
/// lives in the store returns a target URI for a file the IDE has
/// never opened, never indexed, and considers "not in any project."
/// Some navigation paths skip such targets entirely.
///
/// The provider enumerates every `<pkg>@<ver>/lib/` directly rather
/// than parsing `~/.stryke/installed.toml`. Reasons:
///   - Independent of the manifest's name normalization (`stryke-foo`
///     vs `foo` store-dir naming has diverged over versions).
///   - Robust to the user installing a package out-of-band by
///     cloning into the store directly.
///   - Cheap — one `readdir` on the store root + one `lib` stat per
///     entry, run lazily by the platform when the project opens.
class StrykeAdditionalLibraryRootsProvider : AdditionalLibraryRootsProvider() {

    override fun getAdditionalProjectLibraries(project: Project): Collection<SyntheticLibrary> {
        val storeRoot = storeRoot()
        if (!storeRoot.isDirectory) return emptyList()
        val lfs = LocalFileSystem.getInstance()
        val entries = storeRoot.listFiles { f -> f.isDirectory } ?: return emptyList()
        return entries.mapNotNull { pkgDir ->
            val libDir = File(pkgDir, "lib")
            if (!libDir.isDirectory) return@mapNotNull null
            val vfile = lfs.findFileByIoFile(libDir) ?: return@mapNotNull null
            StrykeSyntheticLibrary(pkgDir.name, vfile)
        }
    }

    override fun getRootsToWatch(project: Project): Collection<VirtualFile> {
        val storeRoot = storeRoot()
        if (!storeRoot.isDirectory) return emptyList()
        return listOfNotNull(LocalFileSystem.getInstance().findFileByIoFile(storeRoot))
    }

    private fun storeRoot(): File {
        // Honor STRYKE_HOME the same way the Rust pkg::store::Store does.
        val home = System.getenv("STRYKE_HOME")?.takeIf { it.isNotBlank() }
            ?: System.getProperty("user.home") + "/.stryke"
        return File(home, "store")
    }
}

/// SyntheticLibrary entry per installed store package. The label that
/// shows under "External Libraries" is `stryke: <pkg>@<ver>`. Source
/// root is the package's `lib/` directory — that's where every `.stk`
/// the `use NAME` resolver can land.
///
/// `comparisonId` is set so the platform can incrementally re-scan
/// changes inside the library instead of full-rebuild.
class StrykeSyntheticLibrary(
    private val name: String,
    private val libRoot: VirtualFile,
) : SyntheticLibrary("stryke-store:$name", null), ItemPresentation {

    override fun getSourceRoots(): Collection<VirtualFile> = listOf(libRoot)

    override fun equals(other: Any?): Boolean =
        other is StrykeSyntheticLibrary && other.name == name && other.libRoot == libRoot

    override fun hashCode(): Int = 31 * name.hashCode() + libRoot.hashCode()

    override fun getPresentableText(): String = "stryke: $name"

    override fun getLocationString(): String = libRoot.path

    override fun getIcon(unused: Boolean): Icon = StrykeIcons.FILE
}
