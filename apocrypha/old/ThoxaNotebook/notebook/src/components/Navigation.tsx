import { Link } from 'waku'
import { getNavItems } from '../actions/learn'

export default async function Navigation() {
  const navItems = await getNavItems()

  return (
    <nav className="bg-gray-950 border-b border-gray-800 px-6 py-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center space-x-8">
          <Link
            to="/"
            className="text-xl font-bold text-white hover:text-gray-300 transition-colors"
          >
            Thoxa Notebook
          </Link>
          <div className="flex space-x-6">
            {navItems.map((item) => (
              <Link
                key={item.id}
                to={item.path as Parameters<typeof Link>['0']['to']}
                className="text-gray-300 hover:text-white transition-colors"
              >
                {item.label}
              </Link>
            ))}
          </div>
        </div>
      </div>
    </nav>
  )
}
