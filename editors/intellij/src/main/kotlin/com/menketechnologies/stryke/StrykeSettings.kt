package com.menketechnologies.stryke

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.util.xmlb.XmlSerializerUtil

@Service(Service.Level.APP)
@State(name = "StrykeSettings", storages = [Storage("stryke.xml")])
class StrykeSettings : PersistentStateComponent<StrykeSettings.State> {
    data class State(
        var stExecutable: String? = null,
        var lspEnabled: Boolean = true,
        var extraLspArgs: String = "",
        var defaultNoInterop: Boolean = false,
        var disableLexerHighlighting: Boolean = false,
        var fileExtensions: String = "stk",
        var autoRestartLsp: Boolean = true,
        var lspEnv: String = "",
        var logLspToFile: Boolean = false,
        var lspLogPath: String = "",
        var enableBuiltinHovers: Boolean = true,
    )

    private var stateData = State()

    override fun getState(): State = stateData
    override fun loadState(state: State) { XmlSerializerUtil.copyBean(state, stateData) }

    // Sugar accessors
    var stExecutable: String?
        get() = stateData.stExecutable
        set(value) { stateData.stExecutable = value }
    var lspEnabled: Boolean
        get() = stateData.lspEnabled
        set(value) { stateData.lspEnabled = value }
    var extraLspArgs: String
        get() = stateData.extraLspArgs
        set(value) { stateData.extraLspArgs = value }
    var defaultNoInterop: Boolean
        get() = stateData.defaultNoInterop
        set(value) { stateData.defaultNoInterop = value }
    var disableLexerHighlighting: Boolean
        get() = stateData.disableLexerHighlighting
        set(value) { stateData.disableLexerHighlighting = value }
    var fileExtensions: String
        get() = stateData.fileExtensions
        set(value) { stateData.fileExtensions = value }
    var autoRestartLsp: Boolean
        get() = stateData.autoRestartLsp
        set(value) { stateData.autoRestartLsp = value }
    var lspEnv: String
        get() = stateData.lspEnv
        set(value) { stateData.lspEnv = value }
    var logLspToFile: Boolean
        get() = stateData.logLspToFile
        set(value) { stateData.logLspToFile = value }
    var lspLogPath: String
        get() = stateData.lspLogPath
        set(value) { stateData.lspLogPath = value }
    var enableBuiltinHovers: Boolean
        get() = stateData.enableBuiltinHovers
        set(value) { stateData.enableBuiltinHovers = value }

    fun supportedExtensions(): List<String> =
        fileExtensions.split(",", " ", ";")
            .map { it.trim().removePrefix(".") }
            .filter { it.isNotEmpty() }

    companion object {
        fun getInstance(): StrykeSettings =
            ApplicationManager.getApplication().getService(StrykeSettings::class.java)
    }
}
