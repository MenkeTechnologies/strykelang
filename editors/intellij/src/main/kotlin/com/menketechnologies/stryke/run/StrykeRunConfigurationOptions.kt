package com.menketechnologies.stryke.run

import com.intellij.execution.configurations.RunConfigurationOptions

class StrykeRunConfigurationOptions : RunConfigurationOptions() {
    var scriptPath: String? by string()
    var scriptArgs: String? by string()
    var interpreterArgs: String? by string()
    var workingDirectory: String? by string()
    var noInterop: Boolean by property(false)
}
