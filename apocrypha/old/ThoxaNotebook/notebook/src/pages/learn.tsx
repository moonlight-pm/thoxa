import { getLearnSections, getTopicBySlug } from '../actions/learn'
import LearnPageClient from '../components/LearnPageClient'
import { initializeDatabase } from '../model'

// Initialize database on server startup
await initializeDatabase()

export default async function LearnPage() {
  // Get all sections and topics from database
  const sections = await getLearnSections()

  // Transform data for client component
  const sectionsData = sections.map((section) => ({
    title: section.chapter.title,
    subsections: section.topics.map((topic) => ({
      id: topic.slug,
      title: topic.title,
      description: topic.description || '',
      explanation: topic.explanation || '',
    })),
  }))

  // Load all code snippets for each topic
  const savedCodeSamples: { [key: string]: string | string[] } = {}

  for (const section of sections) {
    for (const topic of section.topics) {
      const topicData = await getTopicBySlug(topic.slug)
      if (topicData && topicData.snippets.length > 0) {
        // If there's only one snippet, store as string
        // If multiple, store as array
        if (topicData.snippets.length === 1) {
          const firstSnippet = topicData.snippets[0]
          savedCodeSamples[topic.slug] = firstSnippet
            ? firstSnippet.code || ''
            : ''
        } else {
          savedCodeSamples[topic.slug] = topicData.snippets.map(
            (s) => s.code || '',
          )
        }
      }
    }
  }

  return (
    <LearnPageClient
      savedCodeSamples={savedCodeSamples}
      sections={sectionsData}
    />
  )
}
