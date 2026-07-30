package com.example.handballshell.db

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase

@Database(
    entities = [TeamRow::class, PlayerRow::class, MatchRow::class, FactRow::class],
    version = 1,
    exportSchema = false,
)
abstract class ShellDatabase : RoomDatabase() {
    abstract fun dao(): ShellDao

    companion object {
        @Volatile
        private var instance: ShellDatabase? = null

        fun get(context: Context): ShellDatabase =
            instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    ShellDatabase::class.java,
                    "handball-shell.db",
                ).build().also { instance = it }
            }
    }
}
