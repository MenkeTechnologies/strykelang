package com.menketechnologies.stryke.run

import com.intellij.execution.actions.ConfigurationContext
import com.intellij.execution.actions.LazyRunConfigurationProducer
import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.openapi.util.Ref
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile

class StrykeRunConfigurationProducer : LazyRunConfigurationProducer<StrykeRunConfiguration>() {

    override fun getConfigurationFactory(): ConfigurationFactory =
        StrykeRunConfigurationType.getInstance().factory

    override fun setupConfigurationFromContext(
        config: StrykeRunConfiguration,
        context: ConfigurationContext,
        sourceElement: Ref<PsiElement>,
    ): Boolean {
        val file: PsiFile = context.psiLocation?.containingFile ?: return false
        val vf = file.virtualFile ?: return false
        if (vf.extension != "stk") return false
        config.options.scriptPath = vf.path
        config.name = vf.nameWithoutExtension
        if (config.options.workingDirectory.isNullOrBlank()) {
            config.options.workingDirectory = vf.parent?.path ?: ""
        }
        return true
    }

    override fun isConfigurationFromContext(
        config: StrykeRunConfiguration,
        context: ConfigurationContext,
    ): Boolean {
        val vf = context.psiLocation?.containingFile?.virtualFile ?: return false
        return vf.extension == "stk" && config.options.scriptPath == vf.path
    }
}
