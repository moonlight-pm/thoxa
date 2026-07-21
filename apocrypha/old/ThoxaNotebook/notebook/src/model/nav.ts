import { type ChapterRecord, db, type NavRecord, updateTimestamp } from './db'

export class NavModel {
  static getAll(): NavRecord[] {
    return db
      .query<NavRecord, []>(
        'SELECT * FROM nav WHERE active = 1 ORDER BY position',
      )
      .all()
  }

  static getById(id: number): NavRecord | null {
    return db
      .query<NavRecord, [number]>('SELECT * FROM nav WHERE id = ?')
      .get(id)
  }

  static getByPath(path: string): NavRecord | null {
    return db
      .query<NavRecord, [string]>('SELECT * FROM nav WHERE path = ?')
      .get(path)
  }

  static create(label: string, path: string, position: number): number {
    const stmt = db.prepare(
      'INSERT INTO nav (label, path, position) VALUES (?, ?, ?)',
    )
    const result = stmt.run(label, path, position)
    return Number(result.lastInsertRowid)
  }

  static update(
    id: number,
    updates: Partial<Omit<NavRecord, 'id' | 'created' | 'updated'>>,
  ): void {
    const fields = Object.keys(updates)
    const values = Object.values(updates)

    if (fields.length === 0) return

    const setClause = fields.map((f) => `${f} = ?`).join(', ')
    const stmt = db.prepare(`UPDATE nav SET ${setClause} WHERE id = ?`)
    stmt.run(...values, id)

    updateTimestamp('nav', id)
  }

  static delete(id: number): void {
    // Soft delete by setting active = 0
    const stmt = db.prepare('UPDATE nav SET active = 0 WHERE id = ?')
    stmt.run(id)
    updateTimestamp('nav', id)
  }

  static getChapters(navId: number): ChapterRecord[] {
    return db
      .query<ChapterRecord, [number]>(
        'SELECT * FROM chapters WHERE nav = ? AND active = 1 ORDER BY position',
      )
      .all(navId)
  }
}
