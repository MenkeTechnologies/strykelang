package com.menketechnologies.stryke.run

import com.intellij.execution.configurations.LocatableRunConfigurationOptions

class StrykeRunConfigurationOptions : LocatableRunConfigurationOptions() {
    var scriptPath: String? by string()
    var scriptArgs: String? by string()
    var interpreterArgs: String? by string()
    var workingDirectory: String? by string()
    var noInterop: Boolean by property(false)
    var disasm: Boolean by property(false)
    var profile: Boolean by property(false)
    var flame: Boolean by property(false)
    var debugFlag: Boolean by property(false)   // -d (Perl-style debugger)
    var debugFlags: String? by string()         // -D switches when in debug mode
}
