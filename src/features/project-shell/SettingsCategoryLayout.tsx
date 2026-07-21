import { useState, type ReactNode } from 'react';

export type SettingsCategory = {
  id: string;
  label: string;
  description?: string;
  content: ReactNode;
};

export function SettingsCategoryLayout({
  categories,
}: {
  categories: SettingsCategory[];
}) {
  const [selectedId, setSelectedId] = useState(categories[0]?.id ?? '');
  const activeId = categories.some(({ id }) => id === selectedId)
    ? selectedId
    : (categories[0]?.id ?? '');

  return (
    <div className="plume-settings-layout">
      <nav className="plume-settings-navigation" aria-label="Settings sections">
        {categories.map((category) => {
          const selected = category.id === activeId;
          return (
            <button
              key={category.id}
              type="button"
              aria-current={selected ? 'page' : undefined}
              aria-controls={`plume-settings-page-${category.id}`}
              onClick={() => setSelectedId(category.id)}
            >
              {category.label}
            </button>
          );
        })}
      </nav>

      <div className="plume-settings-pages">
        {categories.map((category) => {
          const titleId = `plume-settings-page-${category.id}-title`;
          return (
            <section
              key={category.id}
              id={`plume-settings-page-${category.id}`}
              className="plume-settings-page"
              role="region"
              aria-label={category.label}
              hidden={category.id !== activeId}
            >
              <header className="plume-settings-page-header">
                <h4 id={titleId}>{category.label}</h4>
                {category.description ? <p>{category.description}</p> : null}
              </header>
              <div className="plume-settings-page-content">{category.content}</div>
            </section>
          );
        })}
      </div>
    </div>
  );
}
