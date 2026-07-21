import fs from 'node:fs'
import path from 'node:path'

// Store saved code samples in a JSON file
const SAVED_CODE_FILE = path.join(process.cwd(), 'saved-code-samples.json')

export interface SavedCodeSample {
  sectionId: string
  code: string
  savedAt: string
}

// Load saved code samples
export function loadSavedCode(): Record<string, SavedCodeSample> {
  try {
    if (fs.existsSync(SAVED_CODE_FILE)) {
      const data = fs.readFileSync(SAVED_CODE_FILE, 'utf-8')
      return JSON.parse(data)
    }
  } catch (error) {
    console.error('Error loading saved code:', error)
  }
  return {}
}

// Save a code sample
export function saveCodeSample(sectionId: string, code: string): boolean {
  try {
    const savedSamples = loadSavedCode()
    savedSamples[sectionId] = {
      sectionId,
      code,
      savedAt: new Date().toISOString(),
    }

    fs.writeFileSync(SAVED_CODE_FILE, JSON.stringify(savedSamples, null, 2))
    return true
  } catch (error) {
    console.error('Error saving code:', error)
    return false
  }
}

// Server action for saving code
export async function saveCode(
  sectionId: string,
  code: string,
): Promise<{ success: boolean; message: string }> {
  'use server'

  if (!sectionId || !code) {
    return { success: false, message: 'Section ID and code are required' }
  }

  const success = saveCodeSample(sectionId, code)

  if (success) {
    return { success: true, message: 'Code saved successfully' }
  } else {
    return { success: false, message: 'Failed to save code' }
  }
}
