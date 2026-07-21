'use client'

import { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { saveTopicCode } from '../actions/learn'

// Lazily import ThoxaEditor
const ThoxaEditor = lazy(() => import('./ThoxaEditor'))

interface Section {
  title: string
  subsections: {
    id: string
    title: string
    description: string
    explanation: string
  }[]
}

interface LearnPageClientProps {
  savedCodeSamples: { [key: string]: string | string[] }
  sections: Section[]
}

// Section component to render each tutorial section
function Section({
  sectionId,
  title,
  description,
  explanation,
  initialSavedCode,
}: {
  sectionId: string
  title: string
  description: string
  explanation: string
  initialSavedCode: string | string[] | null
}) {
  const [savedCode, setSavedCode] = useState<string | string[] | null>(
    initialSavedCode,
  )

  const handleSave = async (code: string, tabIndex: number = 0) => {
    try {
      // Save to database via server action
      await saveTopicCode(sectionId, tabIndex, code)

      // Update local state
      if (Array.isArray(savedCode)) {
        const newCode = [...savedCode]
        newCode[tabIndex] = code
        setSavedCode(newCode)
      } else {
        setSavedCode(code)
      }
    } catch (error) {
      console.error('Save error:', error)
      throw error
    }
  }

  return (
    <div id={sectionId} className="mb-16 scroll-mt-8">
      <h2 className="text-2xl font-bold mb-2">{title}</h2>
      <p className="text-gray-400 mb-6">{description}</p>

      {/* Interactive Code Editor */}
      <div className="mb-6">
        <Suspense
          fallback={
            <div className="flex items-center justify-center h-64 bg-gray-800 rounded-lg">
              Loading editor...
            </div>
          }
        >
          <ThoxaEditor
            initialCode={(() => {
              const code = savedCode || initialSavedCode || ''
              // Ensure arrays don't contain nulls
              if (Array.isArray(code)) {
                return code.map((c) => c || '')
              }
              return code || ''
            })()}
            showAST={false}
            editable={true}
            sectionId={sectionId}
            onSave={handleSave}
          />
        </Suspense>
      </div>

      {/* Explanation */}
      {explanation && (
        <div className="bg-gray-800 border border-gray-700 rounded-lg p-4">
          <h3 className="font-semibold mb-2 text-gray-200">Explanation</h3>
          <p className="text-gray-400">{explanation}</p>
        </div>
      )}
    </div>
  )
}

export default function LearnPageClient({
  savedCodeSamples,
  sections,
}: LearnPageClientProps) {
  const [activeSection, setActiveSection] = useState('hello-world')
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const sectionObserverRef = useRef<IntersectionObserver | null>(null)

  // Scroll to section when clicked in sidebar
  const handleSectionClick = (sectionId: string) => {
    setActiveSection(sectionId)
    sessionStorage.setItem('learnPageSection', sectionId)
    const element = document.getElementById(sectionId)
    if (element) {
      element.scrollIntoView({ behavior: 'smooth' })
    }
  }

  // Save scroll position on scroll
  useEffect(() => {
    const container = scrollContainerRef.current
    if (!container) return

    const handleScroll = () => {
      sessionStorage.setItem(
        'learnPageScrollTop',
        container.scrollTop.toString(),
      )
    }

    container.addEventListener('scroll', handleScroll)
    return () => container.removeEventListener('scroll', handleScroll)
  }, [])

  // Restore scroll position on mount
  useEffect(() => {
    const savedScrollTop = sessionStorage.getItem('learnPageScrollTop')
    const savedSection = sessionStorage.getItem('learnPageSection')

    // Delay to ensure content is loaded
    setTimeout(() => {
      const container = scrollContainerRef.current
      if (container && savedScrollTop) {
        container.scrollTop = parseInt(savedScrollTop, 10)
      } else if (savedSection) {
        const element = document.getElementById(savedSection)
        if (element) {
          element.scrollIntoView({ behavior: 'instant' })
        }
      }

      if (savedSection) {
        setActiveSection(savedSection)
      }
    }, 100)
  }, [])

  // Set up intersection observer for active section detection
  useEffect(() => {
    const observerOptions = {
      root: null,
      rootMargin: '-20% 0px -60% 0px', // Trigger when section is in upper portion of viewport
      threshold: 0,
    }

    sectionObserverRef.current = new IntersectionObserver(
      (entries: IntersectionObserverEntry[]) => {
        entries.forEach((entry: IntersectionObserverEntry) => {
          if (entry.isIntersecting) {
            const sectionId = entry.target.id
            setActiveSection(sectionId)
            sessionStorage.setItem('learnPageSection', sectionId)
          }
        })
      },
      observerOptions,
    )

    // Observe all sections
    const allSections = sections.flatMap((section) =>
      section.subsections.map((subsection) => subsection.id),
    )

    allSections.forEach((sectionId) => {
      const element = document.getElementById(sectionId)
      if (element && sectionObserverRef.current) {
        sectionObserverRef.current.observe(element)
      }
    })

    // Cleanup
    return () => {
      if (sectionObserverRef.current) {
        sectionObserverRef.current.disconnect()
      }
    }
  }, [sections])

  return (
    <div className="flex h-full">
      {/* Sidebar */}
      <div className="w-64 bg-gray-950 border-r border-gray-800 overflow-y-auto custom-scrollbar flex-shrink-0">
        <div className="p-4">
          <h2 className="text-lg font-bold mb-4 text-gray-100">Learn Thoxa</h2>
          <nav className="space-y-4">
            {sections.map((section, sectionIndex) => (
              <div key={`section-${sectionIndex}`}>
                <h3 className="text-sm font-semibold text-gray-400 mb-2">
                  {section.title}
                </h3>
                <ul className="space-y-1">
                  {section.subsections.map((subsection) => (
                    <li key={subsection.id}>
                      <button
                        type="button"
                        onClick={() => handleSectionClick(subsection.id)}
                        className={`w-full text-left px-3 py-1.5 text-sm rounded transition-colors ${
                          activeSection === subsection.id
                            ? 'bg-gray-800 text-gray-100'
                            : 'text-gray-400 hover:bg-gray-800 hover:text-gray-100'
                        }`}
                      >
                        {subsection.title}
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </nav>
        </div>
      </div>

      {/* Main Content - Single Scrolling Page */}
      <div
        ref={scrollContainerRef}
        className="flex-1 overflow-y-auto custom-scrollbar"
      >
        <div className="max-w-4xl mx-auto p-8">
          <h1 className="text-3xl font-bold mb-8">Learn Thoxa</h1>

          {/* Render all sections in order */}
          {sections.flatMap((section) =>
            section.subsections.map((subsection) => (
              <Section
                key={subsection.id}
                sectionId={subsection.id}
                title={subsection.title}
                description={subsection.description}
                explanation={subsection.explanation}
                initialSavedCode={savedCodeSamples[subsection.id] || null}
              />
            )),
          )}
        </div>
      </div>
    </div>
  )
}
