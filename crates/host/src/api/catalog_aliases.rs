use super::catalog::RouteSpec;

impl RouteSpec {
    pub(crate) fn aliases(&self) -> &'static [&'static str] {
        match self.route {
            "program.get" => &["read program", "получить программу", "прочитать программу"],
            "program.list" => &["list programs", "список программ"],
            "source.list" => &["list sources", "список исходников"],
            "setup.get" => &["read setup", "получить настройку"],
            "workspace.open" => &["open workspace", "открыть рабочее пространство"],
            "source.register" => &["register source", "зарегистрировать исходник"],
            "session.select_worktrees" => &["select worktrees", "выбрать worktree"],
            "program.begin" => &[
                "begin program",
                "create program",
                "start program",
                "начать программу",
                "создать программу",
                "открыть программу",
            ],
            "program.save" => &["save program", "сохранить программу"],
            "program.record_input" => &["program reply", "ответ для программы"],
            "setup.inspect" => &["inspect agents", "проверить agents"],
            "setup.begin" => &["begin setup", "начать настройку"],
            "setup.save" => &["save setup", "сохранить настройку"],
            "setup.record_input" => &["setup reply", "ответ для настройки"],
            "setup.apply" => &["apply setup", "создать agents", "применить настройку"],
            "scope.candidates.context" => &[
                "read scope candidates",
                "candidate context",
                "прочитать кандидаты scope",
            ],
            "scope.candidates.begin" => &[
                "begin scope candidates",
                "plan scopes",
                "начать кандидаты scope",
            ],
            "scope.candidates.save" => &[
                "save scope candidate draft",
                "review scope candidates",
                "сохранить кандидаты scope",
            ],
            "scope.candidates.record_input" => {
                &["scope candidate amendment", "уточнить кандидаты scope"]
            }
            "scope.candidates.refresh" => &[
                "refresh scope candidate context",
                "обновить контекст кандидатов scope",
            ],
            "scope.context" => &["read scope", "прочитать scope"],
            "slice.pipelines" => &["list slice pipelines", "список pipeline slice"],
            "slice.candidates.context" => &["read slice candidates", "кандидаты slice"],
            "slice.context" => &["read slice", "прочитать slice"],
            "scope.open" => &["open scope", "открыть scope"],
            "slice.candidates.save" => &["save slice candidates", "сохранить кандидаты slice"],
            "slice.candidates.input" => &["amend slice candidates", "уточнить кандидаты slice"],
            "slice.candidates.refresh" => &["refresh slice plan", "обновить план slice"],
            "slice.open" => &["open slice", "открыть slice"],
            "slice.result.record" => &["record slice result", "записать результат slice"],
            "slice.pipeline.context" => &["read slice pipeline", "прочитать pipeline slice"],
            "slice.pipeline.begin" => &["begin slice pipeline", "начать pipeline slice"],
            "slice.pipeline.phase.complete" => {
                &["complete pipeline phase", "завершить фазу pipeline"]
            }
            "slice.pipeline.input" => &["record pipeline input", "уточнить фазу pipeline"],
            "slice.pipeline.delivery.escalate" => &[
                "escalate pipeline delivery",
                "переключить pipeline по фазам",
            ],
            "knowledge.context" => &["read durable knowledge", "прочитать durable knowledge"],
            "knowledge.change" => &["read knowledge change", "прочитать изменение knowledge"],
            "knowledge.change_prepare" => &[
                "prepare knowledge change",
                "подготовить изменение knowledge",
            ],
            "knowledge.change_review" => {
                &["review knowledge change", "проверить изменение knowledge"]
            }
            "knowledge.change_publish" => &[
                "publish knowledge change",
                "опубликовать изменение knowledge",
            ],
            "pipeline.knowledge_refresh" => {
                &["refresh pipeline knowledge", "обновить knowledge pipeline"]
            }
            "knowledge.lifecycle" => &["read knowledge lifecycle", "прочитать lifecycle knowledge"],
            "knowledge.unit" => &["read typed knowledge unit", "прочитать knowledge unit"],
            "knowledge.change_begin" => &["begin knowledge change", "начать knowledge change"],
            "knowledge.change_phase_complete" => &[
                "complete knowledge change phase",
                "завершить фазу knowledge change",
            ],
            "knowledge.change_record_input" => {
                &["record knowledge change input", "уточнить knowledge change"]
            }
            "knowledge.change_commit" => &["commit knowledge change", "применить knowledge change"],
            "knowledge.change_settle_effects" => &[
                "settle knowledge change effects",
                "завершить эффекты knowledge change",
            ],
            _ => &[],
        }
    }
}
