export { ChapterModel } from './chapters'
export type {
  ChapterRecord,
  MigrationRecord,
  NavRecord,
  SnippetRecord,
  TopicRecord,
} from './db'
export { db, updateTimestamp } from './db'
export { NavModel } from './nav'
export { SnippetModel } from './snippets'
export { TopicModel } from './topics'

import migration001 from '../db/migrations/001-initial-schema'
// Initialize database and run migrations on first import
import { migrator } from '../db/migrations/migrator'

// Register migrations
migrator.register(migration001)

// Run migrations (this will create tables and seed data if needed)
export async function initializeDatabase(): Promise<void> {
  await migrator.runMigrations()
}
