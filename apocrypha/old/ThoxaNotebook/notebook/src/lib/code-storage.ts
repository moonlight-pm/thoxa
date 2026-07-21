'use server'

import fs from 'node:fs/promises'
import path from 'node:path'
import yaml from 'js-yaml'

const YAML_FILE_PATH = path.join(process.cwd(), 'saved-code-samples.yaml')

export interface SavedCodeSamples {
  [sectionId: string]: string | string[]
}

// Server action to load saved code samples
export async function loadSavedCodeSamples(): Promise<SavedCodeSamples> {
  try {
    const fileContent = await fs.readFile(YAML_FILE_PATH, 'utf-8')
    const samples = yaml.load(fileContent) as SavedCodeSamples
    return samples || {}
  } catch (error) {
    console.error('Error loading saved code samples:', error)
    return {}
  }
}

// Server action to save a code sample
export async function saveCodeSample(
  sectionId: string,
  code: string | string[],
): Promise<boolean> {
  try {
    // Load existing samples
    const samples = await loadSavedCodeSamples()

    // Update the specific section
    samples[sectionId] = code

    // Convert to YAML with nice formatting
    const yamlContent =
      '# Saved code samples for Thoxa tutorial sections\n' +
      '# This file is automatically updated when users save their code changes\n\n' +
      yaml.dump(samples, {
        lineWidth: -1,
        quotingType: '"',
        forceQuotes: false,
      })

    // Write back to file
    await fs.writeFile(YAML_FILE_PATH, yamlContent, 'utf-8')

    return true
  } catch (error) {
    console.error('Error saving code sample:', error)
    return false
  }
}

// Server action to get a specific section's saved code
export async function getSavedCode(
  sectionId: string,
): Promise<string | string[] | null> {
  try {
    const samples = await loadSavedCodeSamples()
    return samples[sectionId] || null
  } catch (error) {
    console.error('Error getting saved code:', error)
    return null
  }
}
