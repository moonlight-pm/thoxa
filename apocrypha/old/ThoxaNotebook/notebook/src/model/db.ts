import { Database } from 'bun:sqlite'
import path from 'node:path'

const DB_PATH = path.join(process.cwd(), 'data.db')

// Create a singleton database connection
class DatabaseConnection {
  private static instance: Database | null = null

  static getInstance(): Database {
    if (!DatabaseConnection.instance) {
      // Create database file if it doesn't exist
      DatabaseConnection.instance = new Database(DB_PATH, { create: true })

      // Enable foreign keys
      DatabaseConnection.instance.exec('PRAGMA foreign_keys = ON')

      // Use DELETE mode to avoid creating WAL files
      DatabaseConnection.instance.exec('PRAGMA journal_mode = DELETE')
    }

    return DatabaseConnection.instance
  }

  static close(): void {
    if (DatabaseConnection.instance) {
      DatabaseConnection.instance.close()
      DatabaseConnection.instance = null
    }
  }
}

export const db = DatabaseConnection.getInstance()

// Helper to update timestamps
export function updateTimestamp(table: string, id: number): void {
  const stmt = db.prepare(
    `UPDATE ${table} SET updated = CURRENT_TIMESTAMP WHERE id = ?`,
  )
  stmt.run(id)
}

// Type definitions for database records
export interface NavRecord {
  id: number
  created: string
  updated: string
  label: string
  path: string
  position: number
  active: number
}

export interface ChapterRecord {
  id: number
  created: string
  updated: string
  nav: number
  title: string
  position: number
  active: number
}

export interface TopicRecord {
  id: number
  created: string
  updated: string
  chapter: number
  title: string
  slug: string
  description: string | null
  explanation: string | null
  position: number
  active: number
}

export interface SnippetRecord {
  id: number
  created: string
  updated: string
  topic: number
  code: string
  position: number
}

export interface MigrationRecord {
  id: number
  created: string
  updated: string
  version: number
  name: string
}
