export default function HomePage() {
  return (
    <div className="p-8">
      <h1 className="text-4xl font-bold mb-4">Thoxa Interactive Notebook</h1>
      <p className="text-xl text-gray-400 mb-8">
        Learn the Thoxa programming language through live, editable examples
      </p>
      <div className="flex gap-4">
        <a
          href="/learn"
          className="px-6 py-3 bg-blue-600 text-white rounded-md no-underline font-medium hover:bg-blue-700 transition-colors"
        >
          Start Learning
        </a>
        <a
          href="/playground"
          className="px-6 py-3 bg-gray-800 text-gray-200 rounded-md no-underline font-medium hover:bg-gray-700 transition-colors"
        >
          Open Playground
        </a>
      </div>
    </div>
  )
}
