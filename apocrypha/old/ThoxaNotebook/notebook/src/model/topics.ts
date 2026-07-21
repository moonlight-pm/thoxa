import { db, type SnippetRecord, type TopicRecord, updateTimestamp } from './db'

export class TopicModel {
  static getAll(): TopicRecord[] {
    return db
      .query<TopicRecord, []>(
        'SELECT * FROM topics WHERE active = 1 ORDER BY position',
      )
      .all()
  }

  static getByChapter(chapterId: number): TopicRecord[] {
    return db
      .query<TopicRecord, [number]>(
        'SELECT * FROM topics WHERE chapter = ? AND active = 1 ORDER BY position',
      )
      .all(chapterId)
  }

  static getById(id: number): TopicRecord | null {
    return db
      .query<TopicRecord, [number]>('SELECT * FROM topics WHERE id = ?')
      .get(id)
  }

  static getBySlug(slug: string): TopicRecord | null {
    return db
      .query<TopicRecord, [string]>('SELECT * FROM topics WHERE slug = ?')
      .get(slug)
  }

  static create(
    chapterId: number,
    title: string,
    slug: string,
    description: string | null,
    explanation: string | null,
    position: number,
  ): number {
    const stmt = db.prepare(
      'INSERT INTO topics (chapter, title, slug, description, explanation, position) VALUES (?, ?, ?, ?, ?, ?)',
    )
    const result = stmt.run(
      chapterId,
      title,
      slug,
      description,
      explanation,
      position,
    )
    return Number(result.lastInsertRowid)
  }

  static update(
    id: number,
    updates: Partial<Omit<TopicRecord, 'id' | 'created' | 'updated'>>,
  ): void {
    const fields = Object.keys(updates)
    const values = Object.values(updates)

    if (fields.length === 0) return

    const setClause = fields.map((f) => `${f} = ?`).join(', ')
    const stmt = db.prepare(`UPDATE topics SET ${setClause} WHERE id = ?`)
    stmt.run(...values, id)

    updateTimestamp('topics', id)
  }

  static delete(id: number): void {
    // Soft delete
    const stmt = db.prepare('UPDATE topics SET active = 0 WHERE id = ?')
    stmt.run(id)
    updateTimestamp('topics', id)
  }

  static getSnippets(topicId: number): SnippetRecord[] {
    return db
      .query<SnippetRecord, [number]>(
        'SELECT * FROM snippets WHERE topic = ? ORDER BY position',
      )
      .all(topicId)
  }

  static getNextTopic(currentTopicId: number): TopicRecord | null {
    const current = TopicModel.getById(currentTopicId)
    if (!current) return null

    // Try to get next topic in same chapter
    let next = db
      .query<TopicRecord, [number, number]>(
        'SELECT * FROM topics WHERE chapter = ? AND position > ? AND active = 1 ORDER BY position LIMIT 1',
      )
      .get(current.chapter, current.position)

    if (!next) {
      // Get first topic of next chapter
      const nextChapter = db
        .query<{ id: number }, [number]>(
          `SELECT c2.id FROM chapters c1 
         JOIN chapters c2 ON c1.nav = c2.nav 
         WHERE c1.id = ? AND c2.position > c1.position AND c2.active = 1 
         ORDER BY c2.position LIMIT 1`,
        )
        .get(current.chapter)

      if (nextChapter) {
        next = db
          .query<TopicRecord, [number]>(
            'SELECT * FROM topics WHERE chapter = ? AND active = 1 ORDER BY position LIMIT 1',
          )
          .get(nextChapter.id)
      }
    }

    return next
  }

  static getPreviousTopic(currentTopicId: number): TopicRecord | null {
    const current = TopicModel.getById(currentTopicId)
    if (!current) return null

    // Try to get previous topic in same chapter
    let prev = db
      .query<TopicRecord, [number, number]>(
        'SELECT * FROM topics WHERE chapter = ? AND position < ? AND active = 1 ORDER BY position DESC LIMIT 1',
      )
      .get(current.chapter, current.position)

    if (!prev) {
      // Get last topic of previous chapter
      const prevChapter = db
        .query<{ id: number }, [number]>(
          `SELECT c2.id FROM chapters c1 
         JOIN chapters c2 ON c1.nav = c2.nav 
         WHERE c1.id = ? AND c2.position < c1.position AND c2.active = 1 
         ORDER BY c2.position DESC LIMIT 1`,
        )
        .get(current.chapter)

      if (prevChapter) {
        prev = db
          .query<TopicRecord, [number]>(
            'SELECT * FROM topics WHERE chapter = ? AND active = 1 ORDER BY position DESC LIMIT 1',
          )
          .get(prevChapter.id)
      }
    }

    return prev
  }
}
