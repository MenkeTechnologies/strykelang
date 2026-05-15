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
    )

    private var stateData = State()

    var stExecutable: String?
        get() = stateData.stExecutable
        set(value) { stateData.stExecutable = value }

    override fun getState(): State = stateData
    override fun loadState(state: State) { XmlSerializerUtil.copyBean(state, stateData) }

    companion object {
        fun getInstance(): StrykeSettings =
            ApplicationManager.getApplication().getService(StrykeSettings::class.java)
    }
}
