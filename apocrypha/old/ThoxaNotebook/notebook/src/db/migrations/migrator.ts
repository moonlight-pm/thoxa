import { db, type MigrationRecord } from '../../model/db'

export interface Migration {
  version: number
  name: string
  up: () => void
  down?: () => void
}

export class Migrator {
  private migrations: Migration[] = []

  constructor() {
    this.loadMigrations()
  }

  private loadMigrations(): void {
    // Migrations are manually registered for now
    // In production, you'd dynamically import these
    this.migrations = []
  }

  register(migration: Migration): void {
    this.migrations.push(migration)
    this.migrations.sort((a, b) => a.version - b.version)
  }

  async runMigrations(): Promise<void> {
    // Check if migrations table exists
    const tableExists = db
      .query(`
      SELECT name FROM sqlite_master
      WHERE type='table' AND name='migrations'
    `)
      .get()

    if (!tableExists) {
      // The first migration will create the schema
      // Just create the migrations table here
      db.run(`
				CREATE TABLE migrations (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					created DATETIME DEFAULT CURRENT_TIMESTAMP,
					updated DATETIME DEFAULT CURRENT_TIMESTAMP,
					version INTEGER UNIQUE NOT NULL,
					name TEXT NOT NULL
				)
			`)
    }

    // Get applied migrations
    const appliedMigrations = db
      .query<MigrationRecord, []>('SELECT * FROM migrations ORDER BY version')
      .all()

    const appliedVersions = new Set(appliedMigrations.map((m) => m.version))

    // Run pending migrations
    for (const migration of this.migrations) {
      if (!appliedVersions.has(migration.version)) {
        console.log(`Running migration ${migration.version}: ${migration.name}`)

        try {
          db.transaction(() => {
            migration.up()

            // Record migration
            const stmt = db.prepare(
              'INSERT INTO migrations (version, name) VALUES (?, ?)',
            )
            stmt.run(migration.version, migration.name)
          })()

          console.log(`Migration ${migration.version} completed`)
        } catch (error) {
          console.error(`Migration ${migration.version} failed:`, error)
          throw error
        }
      }
    }
  }
}

export const migrator = new Migrator()
