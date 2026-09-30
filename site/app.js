(() => {
  const q = (selector) => document.querySelector(selector);
  const tabs = [...document.querySelectorAll('[role="tab"]')];
  const form = q('#configuration');
  const code = q('#command-preview code');
  const copyButton = q('#copy-command');
  const copyLabel = q('#copy-label');
  const copyStatus = q('#copy-status');
  const fields = {
    host: q('#host'), protocol: q('#protocol'), round: q('#round'), skip: q('#skip-pathping'),
    target: q('#iperf-target'), port: q('#iperf-port'), throughputProtocol: q('#throughput-protocol'),
    maxTests: q('#max-tests'), action: q('#tuning-action'), profile: q('#tuning-profile'), udp: q('#udp-port'),
  };
  let active = 'Triage';
  let plan;

  function render() {
    const values = Object.fromEntries(Object.entries(fields).map(([key, field]) => [key, key === 'skip' ? field.checked : field.value]));
    plan = LanternPlanner.buildPlan(active, values);
    q('#workflow-title').textContent = plan.title;
    q('#workflow-description').textContent = plan.description;
    q('#path-controls').hidden = !plan.path;
    q('#throughput-controls').hidden = !plan.throughput;
    q('#tuning-controls').hidden = !plan.tuning;
    q('#host-label').textContent = `${values.protocol} host`;
    q('#context-note').textContent = plan.context;
    q('#workflow-guide').href = plan.guide;
    q('#tuning-note').textContent = values.action === 'Restore' ? 'An existing backup is required for execution. Dry run plans recovery without reading it.' : 'The generated command previews this action without changing Windows settings.';
    for (const field of Object.values(fields)) {
      const message = plan.errors[field.id];
      field.setAttribute('aria-invalid', String(Boolean(message)));
      const error = q(`#${field.id}-error`);
      if (error) { error.textContent = message || ''; error.hidden = !message; }
    }
    q('#plan-steps').replaceChildren(...plan.steps.map((step) => {
      const item = document.createElement('li');
      const title = document.createElement('strong');
      const description = document.createElement('p');
      title.textContent = step.title;
      description.textContent = step.description;
      item.append(title, description);
      return item;
    }));
    const errorCount = Object.keys(plan.errors).length;
    q('#validation-summary').hidden = !errorCount;
    q('#validation-summary').textContent = errorCount ? `Check ${errorCount === 1 ? 'the highlighted field' : `the ${errorCount} highlighted fields`} to generate a command.` : '';
    code.textContent = plan.command || 'Your command will appear here when the highlighted fields are valid.';
    copyButton.disabled = !plan.command;
    copyLabel.textContent = 'Copy command';
    copyStatus.textContent = '';
  }

  function choose(tab) {
    active = tab.dataset.workflow;
    tabs.forEach((item) => {
      item.setAttribute('aria-selected', String(item === tab));
      item.tabIndex = item === tab ? 0 : -1;
    });
    q('#workflow-panel').setAttribute('aria-labelledby', tab.id);
    render();
  }

  tabs.forEach((tab, index) => {
    tab.addEventListener('click', () => choose(tab));
    tab.addEventListener('keydown', (event) => {
      const keys = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1, Home: -index, End: tabs.length - 1 - index };
      if (!(event.key in keys)) return;
      event.preventDefault();
      const next = tabs[(index + keys[event.key] + tabs.length) % tabs.length];
      choose(next);
      next.focus();
      next.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    });
  });
  const horizontalTabs = matchMedia('(max-width: 1160px)');
  const orientTabs = () => q('.workflow-tabs').setAttribute('aria-orientation', horizontalTabs.matches ? 'horizontal' : 'vertical');
  horizontalTabs.addEventListener('change', orientTabs);
  orientTabs();
  form.addEventListener('input', render);
  form.addEventListener('change', render);
  q('#reset').addEventListener('click', () => {
    for (const [key, field] of Object.entries(fields)) {
      if (key === 'skip') field.checked = LanternPlanner.defaults.skip;
      else field.value = LanternPlanner.defaults[key];
    }
    render();
  });
  copyButton.addEventListener('click', async () => {
    if (!plan.command) return;
    const command = plan.command;
    copyLabel.textContent = 'Copying…';
    copyButton.disabled = true;
    try {
      if (!navigator.clipboard?.writeText) throw new Error('Clipboard unavailable');
      await navigator.clipboard.writeText(command);
      if (plan.command === command) {
        copyLabel.textContent = 'Copied';
        copyStatus.textContent = 'Dry-run command copied. Paste it in a terminal with the Rust CLI installed.';
      } else {
        copyStatus.textContent = 'The previous command was copied. Copy again for your updated settings.';
      }
    } catch {
      copyLabel.textContent = 'Copy command';
      q('#command-preview').focus();
      const range = document.createRange();
      range.selectNodeContents(code);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      copyStatus.textContent = 'Copy is unavailable here. The command is selected; copy it manually.';
    } finally {
      copyButton.disabled = !plan.command;
    }
  });
  render();
})();
