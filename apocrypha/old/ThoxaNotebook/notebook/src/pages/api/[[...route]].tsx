import fs from 'node:fs'
import path from 'node:path'

// Store saved code samples in a JSON file
const SAVED_CODE_FILE = path.join(process.cwd(), 'saved-code-samples.json')

interface SavedCodeSample {
  sectionId: string
  code: string
  savedAt: string
}

// Load saved code samples
function loadSavedCode(): Record<string, SavedCodeSample> {
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
function saveCodeSample(sectionId: string, code: string): boolean {
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

export default async function ApiRoute({
  request,
  params,
}: {
  request: Request
  params: { route: string[] }
}) {
  const route = params.route?.join('/') || ''

  // Handle save-code endpoint
  if (route === 'save-code' && request.method === 'POST') {
    try {
      const body = (await request.json()) as {
        sectionId?: string
        code?: string
      }
      const { sectionId, code } = body

      if (!sectionId || !code) {
        return new Response(
          JSON.stringify({
            success: false,
            message: 'Section ID and code are required',
          }),
          { status: 400, headers: { 'Content-Type': 'application/json' } },
        )
      }

      const success = saveCodeSample(sectionId, code)

      if (success) {
        return new Response(
          JSON.stringify({ success: true, message: 'Code saved successfully' }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        )
      } else {
        return new Response(
          JSON.stringify({ success: false, message: 'Failed to save code' }),
          { status: 500, headers: { 'Content-Type': 'application/json' } },
        )
      }
    } catch {
      return new Response(
        JSON.stringify({ success: false, message: 'Invalid request' }),
        { status: 400, headers: { 'Content-Type': 'application/json' } },
      )
    }
  }

  // Handle load-code endpoint
  if (route.startsWith('load-code/') && request.method === 'GET') {
    const sectionId = route.replace('load-code/', '')
    const savedSamples = loadSavedCode()
    const savedSample = savedSamples[sectionId]

    if (savedSample) {
      return new Response(
        JSON.stringify({
          code: savedSample.code,
          savedAt: savedSample.savedAt,
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      )
    } else {
      return new Response(JSON.stringify({ code: null }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }
  }

  // Not found
  return new Response('Not found', { status: 404 })
}
