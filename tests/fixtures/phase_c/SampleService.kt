package com.example.kotlin

import com.example.kotlin.KotlinHelper

interface IWorker {
    fun work()
}

data class UserInfo(val id: String, val name: String)

object AppConfig {
    val version: String = "1.0"
}

class TaskManager(val name: String) : IWorker {
    override fun work() {
        executeTask()
    }

    fun executeTask() {
        val helper = KotlinHelper()
        helper.assist()
    }
}

fun standaloneTask() {
    val manager = TaskManager("main")
    manager.work()
}
