import { db, type SnippetRecord, type TopicRecord, updateTimestamp } from './db'

export class SnippetModel {
  static getByTopic(topicId: number): SnippetRecord[] {
    return db
      .query<SnippetRecord, [number]>(
        'SELECT * FROM snippets WHERE topic = ? ORDER BY position',
      )
      .all(topicId)
  }

  static getById(id: number): SnippetRecord | null {
    return db
      .query<SnippetRecord, [number]>('SELECT * FROM snippets WHERE id = ?')
      .get(id)
  }

  static create(topicId: number, code: string, position: number): number {
    const stmt = db.prepare(
      'INSERT INTO snippets (topic, code, position) VALUES (?, ?, ?)',
    )
    const result = stmt.run(topicId, code, position)
    return Number(result.lastInsertRowid)
  }

  static update(id: number, code: string): void {
    const stmt = db.prepare('UPDATE snippets SET code = ? WHERE id = ?')
    stmt.run(code, id)
    updateTimestamp('snippets', id)
  }

  static updateByTopicAndPosition(
    topicId: number,
    position: number,
    code: string,
  ): void {
    const stmt = db.prepare(
      'UPDATE snippets SET code = ? WHERE topic = ? AND position = ?',
    )
    stmt.run(code, topicId, position)

    // Update timestamp for the affected snippet
    const snippet = db
      .query<SnippetRecord, [number, number]>(
        'SELECT id FROM snippets WHERE topic = ? AND position = ?',
      )
      .get(topicId, position)

    if (snippet) {
      updateTimestamp('snippets', snippet.id)
    }
  }

  static delete(id: number): void {
    const stmt = db.prepare('DELETE FROM snippets WHERE id = ?')
    stmt.run(id)
  }

  static deleteByTopic(topicId: number): void {
    const stmt = db.prepare('DELETE FROM snippets WHERE topic = ?')
    stmt.run(topicId)
  }

  static addTab(topicId: number, code: string = ''): number {
    // Get the max position for this topic
    const maxPos = db
      .query<{ max_pos: number | null }, [number]>(
        'SELECT MAX(position) as max_pos FROM snippets WHERE topic = ?',
      )
      .get(topicId)

    const newPosition = (maxPos?.max_pos || 0) + 1
    return SnippetModel.create(topicId, code, newPosition)
  }

  static removeTab(topicId: number, position: number): void {
    db.transaction(() => {
      // Delete the tab
      const deleteStmt = db.prepare(
        'DELETE FROM snippets WHERE topic = ? AND position = ?',
      )
      deleteStmt.run(topicId, position)

      // Reorder remaining tabs
      const updateStmt = db.prepare(
        'UPDATE snippets SET position = position - 1 WHERE topic = ? AND position > ?',
      )
      updateStmt.run(topicId, position)

      // Update timestamps for affected snippets
      const affectedSnippets = db
        .query<SnippetRecord, [number, number]>(
          'SELECT id FROM snippets WHERE topic = ? AND position >= ?',
        )
        .all(topicId, position)

      affectedSnippets.forEach((snippet) => {
        updateTimestamp('snippets', snippet.id)
      })
    })()
  }

  static reorderTabs(_topicId: number, snippetIds: number[]): void {
    const stmt = db.prepare('UPDATE snippets SET position = ? WHERE id = ?')

    db.transaction(() => {
      snippetIds.forEach((id, index) => {
        stmt.run(index + 1, id)
        updateTimestamp('snippets', id)
      })
    })()
  }

  static saveUserCode(topicSlug: string, tabIndex: number, code: string): void {
    // Get the topic by slug
    const topic = db
      .query<TopicRecord, [string]>('SELECT id FROM topics WHERE slug = ?')
      .get(topicSlug)

    if (!topic) {
      throw new Error(`Topic with slug '${topicSlug}' not found`)
    }

    // Update the snippet at the given position
    SnippetModel.updateByTopicAndPosition(topic.id, tabIndex + 1, code)
  }
}
