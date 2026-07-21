'use server'

import type {
  ChapterRecord,
  NavRecord,
  SnippetRecord,
  TopicRecord,
} from '../model'
import { ChapterModel, NavModel, SnippetModel, TopicModel } from '../model'

export interface LearnSection {
  chapter: ChapterRecord
  topics: TopicRecord[]
}

export interface TopicWithSnippets {
  topic: TopicRecord
  snippets: SnippetRecord[]
  nextTopic?: TopicRecord | null
  previousTopic?: TopicRecord | null
}

export async function getLearnNav(): Promise<NavRecord | null> {
  return NavModel.getByPath('/learn')
}

export async function getLearnSections(): Promise<LearnSection[]> {
  const learnNav = await getLearnNav()
  if (!learnNav) return []

  const chapters = ChapterModel.getByNav(learnNav.id)

  return chapters.map((chapter) => ({
    chapter,
    topics: TopicModel.getByChapter(chapter.id),
  }))
}

export async function getTopicBySlug(
  slug: string,
): Promise<TopicWithSnippets | null> {
  const topic = TopicModel.getBySlug(slug)
  if (!topic) return null

  const snippets = SnippetModel.getByTopic(topic.id)
  const nextTopic = TopicModel.getNextTopic(topic.id)
  const previousTopic = TopicModel.getPreviousTopic(topic.id)

  return {
    topic,
    snippets,
    nextTopic,
    previousTopic,
  }
}

export async function saveTopicCode(
  topicSlug: string,
  tabIndex: number,
  code: string,
): Promise<void> {
  SnippetModel.saveUserCode(topicSlug, tabIndex, code)
}

export async function addSnippetTab(topicSlug: string): Promise<number> {
  const topic = TopicModel.getBySlug(topicSlug)
  if (!topic) {
    throw new Error(`Topic with slug '${topicSlug}' not found`)
  }

  return SnippetModel.addTab(topic.id, '# New tab\n')
}

export async function removeSnippetTab(
  topicSlug: string,
  tabPosition: number,
): Promise<void> {
  const topic = TopicModel.getBySlug(topicSlug)
  if (!topic) {
    throw new Error(`Topic with slug '${topicSlug}' not found`)
  }

  SnippetModel.removeTab(topic.id, tabPosition)
}

export async function getNavItems(): Promise<NavRecord[]> {
  return NavModel.getAll()
}
