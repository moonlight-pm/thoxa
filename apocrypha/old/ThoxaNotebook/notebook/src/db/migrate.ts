#!/usr/bin/env bun

import { initializeDatabase } from '../model'

console.log('Running database migrations...')

try {
  await initializeDatabase()
  console.log('Migrations completed successfully!')
  process.exit(0)
} catch (error) {
  console.error('Migration failed:', error)
  process.exit(1)
}
