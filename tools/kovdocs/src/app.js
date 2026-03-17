// kovdocs client-side: search, scroll spy, copy buttons, keyboard shortcuts

let searchIndex = null;

document.addEventListener('keydown', e => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
    e.preventDefault();
    toggleSearch();
  }
  if (e.key === 'Escape') {
    document.getElementById('search-overlay').style.display = 'none';
  }
});

function toggleSearch() {
  const overlay = document.getElementById('search-overlay');
  if (overlay.style.display === 'none') {
    overlay.style.display = 'flex';
    document.getElementById('search-input').focus();
    if (!searchIndex) {
      fetch('assets/search-index.json')
        .then(r => r.json())
        .then(data => { searchIndex = data; })
        .catch(() => {});
    }
  } else {
    overlay.style.display = 'none';
  }
}

document.getElementById('search-overlay')?.addEventListener('click', e => {
  if (e.target.id === 'search-overlay') {
    e.target.style.display = 'none';
  }
});

function doSearch(query) {
  const results = document.getElementById('search-results');
  if (!searchIndex || !query || query.length < 2) {
    results.innerHTML = '';
    return;
  }
  const q = query.toLowerCase();
  const matches = searchIndex.filter(entry =>
    entry.title.toLowerCase().includes(q) || entry.body.toLowerCase().includes(q)
  ).slice(0, 8);

  results.innerHTML = matches.map(m =>
    `<a href="${m.url}"><div class="sr-title">${m.title}</div><div class="sr-body">${m.body.substring(0, 100)}...</div></a>`
  ).join('');
}

function copyCode(btn) {
  const pre = btn.closest('.code-block').querySelector('pre code');
  navigator.clipboard.writeText(pre.textContent);
  btn.textContent = 'Copied!';
  setTimeout(() => { btn.textContent = 'Copy'; }, 2000);
}

function openPlayground(btn) {
  const code = btn.closest('.playground').querySelector('pre code').textContent;
  window.open('https://kov.dev/playground#' + encodeURIComponent(code), '_blank');
}

// scroll spy for table of contents
const tocLinks = document.querySelectorAll('.toc a');
const headings = [];
tocLinks.forEach(link => {
  const id = link.getAttribute('href')?.slice(1);
  if (id) {
    const el = document.getElementById(id);
    if (el) headings.push({ el, link });
  }
});

if (headings.length > 0) {
  const observer = new IntersectionObserver(entries => {
    entries.forEach(entry => {
      if (entry.isIntersecting) {
        tocLinks.forEach(l => l.parentElement.classList.remove('active'));
        const match = headings.find(h => h.el === entry.target);
        if (match) match.link.parentElement.classList.add('active');
      }
    });
  }, { rootMargin: '-80px 0px -60% 0px' });

  headings.forEach(h => observer.observe(h.el));
}
