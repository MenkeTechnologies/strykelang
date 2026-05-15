package com.menketechnologies.stryke.run

import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.ConfigurationType
import com.intellij.execution.configurations.RunConfiguration
import com.intellij.openapi.project.Project
import com.menketechnologies.stryke.StrykeIcons
import javax.swing.Icon

class StrykeRunConfigurationType : ConfigurationType {
    override fun getDisplayName(): String = "Stryke"
    override fun getConfigurationTypeDescription(): String = "Run a stryke (.stk) script"
    override fun getIcon(): Icon = StrykeIcons.FILE
    override fun getId(): String = "STRYKE_RUN_CONFIGURATION"
    override fun getConfigurationFactories(): Array<ConfigurationFactory> = arrayOf(factory)

    val factory = object : ConfigurationFactory(this) {
        override fun getId(): String = "Stryke"
        override fun createTemplateConfiguration(project: Project): RunConfiguration =
            StrykeRunConfiguration(project, this, "Stryke")
        override fun getOptionsClass(): Class<StrykeRunConfigurationOptions> =
            StrykeRunConfigurationOptions::class.java
    }

    companion object {
        fun getInstance(): StrykeRunConfigurationType =
            com.intellij.execution.configurations.ConfigurationTypeUtil
                .findConfigurationType(StrykeRunConfigurationType::class.java)
    }
}
