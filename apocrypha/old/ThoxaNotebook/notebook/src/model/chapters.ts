import { type ChapterRecord, db, type TopicRecord, updateTimestamp } from './db'

export class ChapterModel {
  static getAll(): ChapterRecord[] {
    return db
      .query<ChapterRecord, []>(
        'SELECT * FROM chapters WHERE active = 1 ORDER BY position',
      )
      .all()
  }

  static getByNav(navId: number): ChapterRecord[] {
    return db
      .query<ChapterRecord, [number]>(
        'SELECT * FROM chapters WHERE nav = ? AND active = 1 ORDER BY position',
      )
      .all(navId)
  }

  static getById(id: number): ChapterRecord | null {
    return db
      .query<ChapterRecord, [number]>('SELECT * FROM chapters WHERE id = ?')
      .get(id)
  }

  static create(navId: number, title: string, position: number): number {
    const stmt = db.prepare(
      'INSERT INTO chapters (nav, title, position) VALUES (?, ?, ?)',
    )
    const result = stmt.run(navId, title, position)
    return Number(result.lastInsertRowid)
  }

  static update(
    id: number,
    updates: Partial<Omit<ChapterRecord, 'id' | 'created' | 'updated'>>,
  ): void {
    const fields = Object.keys(updates)
    const values = Object.values(updates)

    if (fields.length === 0) return

    const setClause = fields.map((f) => `${f} = ?`).join(', ')
    const stmt = db.prepare(`UPDATE chapters SET ${setClause} WHERE id = ?`)
    stmt.run(...values, id)

    updateTimestamp('chapters', id)
  }

  static delete(id: number): void {
    // Soft delete
    const stmt = db.prepare('UPDATE chapters SET active = 0 WHERE id = ?')
    stmt.run(id)
    updateTimestamp('chapters', id)
  }

  static getTopics(chapterId: number): TopicRecord[] {
    return db
      .query<TopicRecord, [number]>(
        'SELECT * FROM topics WHERE chapter = ? AND active = 1 ORDER BY position',
      )
      .all(chapterId)
  }

  static reorder(chapterIds: number[]): void {
    const stmt = db.prepare('UPDATE chapters SET position = ? WHERE id = ?')

    db.transaction(() => {
      chapterIds.forEach((id, index) => {
        stmt.run(index + 1, id)
        updateTimestamp('chapters', id)
      })
    })()
  }
}
