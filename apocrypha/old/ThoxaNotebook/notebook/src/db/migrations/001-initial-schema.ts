import { readFileSync } from 'node:fs'
import path from 'node:path'
import yaml from 'js-yaml'
import { db } from '../../model/db'
import type { Migration } from './migrator'

const migration: Migration = {
  version: 1,
  name: 'initial-schema-and-seed',

  up() {
    // Create all tables
    const schema = `
      -- Navigation items (top nav bar)
      CREATE TABLE IF NOT EXISTS nav (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        created DATETIME DEFAULT CURRENT_TIMESTAMP,
        updated DATETIME DEFAULT CURRENT_TIMESTAMP,
        label TEXT NOT NULL,
        path TEXT NOT NULL,
        position INTEGER NOT NULL,
        active BOOLEAN DEFAULT 1
      );

      -- Tutorial chapters (e.g., "1. Basics", "2. Types", etc.)
      CREATE TABLE IF NOT EXISTS chapters (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        created DATETIME DEFAULT CURRENT_TIMESTAMP,
        updated DATETIME DEFAULT CURRENT_TIMESTAMP,
        nav INTEGER NOT NULL,
        title TEXT NOT NULL,
        position INTEGER NOT NULL,
        active BOOLEAN DEFAULT 1,
        FOREIGN KEY (nav) REFERENCES nav(id)
      );

      -- Tutorial topics (individual lessons like "Hello World", "Variables", etc.)
      CREATE TABLE IF NOT EXISTS topics (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        created DATETIME DEFAULT CURRENT_TIMESTAMP,
        updated DATETIME DEFAULT CURRENT_TIMESTAMP,
        chapter INTEGER NOT NULL,
        title TEXT NOT NULL,
        slug TEXT UNIQUE NOT NULL,
        description TEXT,
        explanation TEXT,
        position INTEGER NOT NULL,
        active BOOLEAN DEFAULT 1,
        FOREIGN KEY (chapter) REFERENCES chapters(id)
      );

      -- Code snippets for each topic (supports multiple tabs)
      CREATE TABLE IF NOT EXISTS snippets (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        created DATETIME DEFAULT CURRENT_TIMESTAMP,
        updated DATETIME DEFAULT CURRENT_TIMESTAMP,
        topic INTEGER NOT NULL,
        code TEXT NOT NULL,
        position INTEGER NOT NULL,
        FOREIGN KEY (topic) REFERENCES topics(id)
      );
    `

    db.run(schema)

    // Load YAML data
    const yamlPath = path.join(process.cwd(), 'saved-code-samples.yaml')
    const yamlContent = readFileSync(yamlPath, 'utf-8')
    const savedSamples = yaml.load(yamlContent) as Record<
      string,
      string | string[]
    >

    // Insert navigation items
    const navStmt = db.prepare(
      'INSERT INTO nav (label, path, position, active) VALUES (?, ?, ?, ?)',
    )

    const learnNavId = Number(
      navStmt.run('Learn', '/learn', 1, 1).lastInsertRowid,
    )
    navStmt.run('Playground', '/playground', 2, 1)
    navStmt.run('Examples', '/examples', 3, 1)

    // Insert chapters
    const chapterStmt = db.prepare(
      'INSERT INTO chapters (nav, title, position, active) VALUES (?, ?, ?, ?)',
    )

    const basicsChapterId = Number(
      chapterStmt.run(learnNavId, '1. Basics', 1, 1).lastInsertRowid,
    )
    const typesChapterId = Number(
      chapterStmt.run(learnNavId, '2. Types', 2, 1).lastInsertRowid,
    )
    const functionsChapterId = Number(
      chapterStmt.run(learnNavId, '3. Functions', 3, 1).lastInsertRowid,
    )
    const controlFlowChapterId = Number(
      chapterStmt.run(learnNavId, '4. Control Flow', 4, 1).lastInsertRowid,
    )

    // Define topics structure
    const topicsData = [
      // Basics chapter
      {
        chapter: basicsChapterId,
        slug: 'hello-world',
        title: 'Hello World',
        description: 'Your first Thoxa program',
        explanation:
          "Learn how to write and run your first Thoxa program. Unlike many languages, Thoxa doesn't require a main() function!",
        position: 1,
      },
      {
        chapter: basicsChapterId,
        slug: 'comments',
        title: 'Comments',
        description: 'Document your code',
        explanation:
          'Comments help explain your code. Use # for single-line comments.',
        position: 2,
      },
      {
        chapter: basicsChapterId,
        slug: 'variables',
        title: 'Variables',
        description: 'Store and use data',
        explanation:
          "Variables store data. Use `let` for immutable variables that can't be changed after creation.",
        position: 3,
      },
      {
        chapter: basicsChapterId,
        slug: 'mutability',
        title: 'Mutability',
        description: 'Variables that can change',
        explanation:
          'Use `mut` to create mutable variables that can be modified after creation.',
        position: 4,
      },

      // Types chapter
      {
        chapter: typesChapterId,
        slug: 'basic-types',
        title: 'Basic Types',
        description: 'Numbers, strings, and booleans',
        explanation:
          'Thoxa has built-in types for integers, floats, strings, and booleans. Type annotations are optional when the type can be inferred.',
        position: 1,
      },
      {
        chapter: typesChapterId,
        slug: 'arrays',
        title: 'Arrays',
        description: 'Collections of values',
        explanation:
          'Arrays store multiple values of the same type. Access elements by index and get the length with len().',
        position: 2,
      },
      {
        chapter: typesChapterId,
        slug: 'custom-types',
        title: 'Custom Types',
        description: 'Define your own types',
        explanation:
          'Create custom types with fields and methods. Use `export` to make fields accessible.',
        position: 3,
      },

      // Functions chapter
      {
        chapter: functionsChapterId,
        slug: 'defining-functions',
        title: 'Defining Functions',
        description: 'Create reusable code',
        explanation:
          'Functions let you organize code into reusable blocks. Parameters require type annotations.',
        position: 1,
      },
      {
        chapter: functionsChapterId,
        slug: 'parameters',
        title: 'Parameters & Returns',
        description: 'Input and output',
        explanation:
          'Functions can take parameters and return values. The last expression is automatically returned.',
        position: 2,
      },
      {
        chapter: functionsChapterId,
        slug: 'methods',
        title: 'Methods',
        description: 'Functions in types',
        explanation:
          "Types can have methods that operate on their data. Methods have access to the type's fields.",
        position: 3,
      },

      // Control Flow chapter
      {
        chapter: controlFlowChapterId,
        slug: 'if-else',
        title: 'If-Else',
        description: 'Conditional execution',
        explanation:
          'Use if-else for conditional logic. If expressions can return values.',
        position: 1,
      },
      {
        chapter: controlFlowChapterId,
        slug: 'while-loops',
        title: 'While Loops',
        description: 'Repeat while condition is true',
        explanation:
          'While loops run as long as their condition is true. Perfect for counters and countdowns.',
        position: 2,
      },
      {
        chapter: controlFlowChapterId,
        slug: 'for-loops',
        title: 'For Loops',
        description: 'Iterate over collections',
        explanation:
          'For loops iterate over arrays and other collections. The loop variable is automatically created.',
        position: 3,
      },
    ]

    // Insert topics
    const topicStmt = db.prepare(
      'INSERT INTO topics (chapter, title, slug, description, explanation, position, active) VALUES (?, ?, ?, ?, ?, ?, ?)',
    )

    const topicIds: Record<string, number> = {}

    for (const topic of topicsData) {
      const result = topicStmt.run(
        topic.chapter,
        topic.title,
        topic.slug,
        topic.description,
        topic.explanation,
        topic.position,
        1,
      )
      topicIds[topic.slug] = Number(result.lastInsertRowid)
    }

    // Insert code snippets from YAML
    const snippetStmt = db.prepare(
      'INSERT INTO snippets (topic, code, position) VALUES (?, ?, ?)',
    )

    for (const [slug, codeData] of Object.entries(savedSamples)) {
      const topicId = topicIds[slug]
      if (topicId) {
        const codes = Array.isArray(codeData) ? codeData : [codeData]

        codes.forEach((code, index) => {
          snippetStmt.run(topicId, code.trim(), index + 1)
        })
      }
    }
  },
}

export default migration
