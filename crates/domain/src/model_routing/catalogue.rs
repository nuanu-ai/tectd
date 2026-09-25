impl ModelRouteCatalogue {
    pub fn validate(&self) -> Result<()> {
        if self.schema != MODEL_ROUTE_CATALOGUE_SCHEMA
            || self.version == 0
            || self.routes.len() > 64
        {
            return Err(Error::InvalidArguments);
        }
        let mut ids = BTreeSet::new();
        for route in &self.routes {
            if !valid_id(&route.id)
                || !valid_id(&route.provider)
                || !valid_id(&route.model)
                || !valid_id(&route.effort)
                || !ids.insert(&route.id)
            {
                return Err(Error::InvalidArguments);
            }
            valid_set(&route.allowed_roles, false)?;
            valid_matrix_choice_set(&route.allowed_matrix_choice_ids)?;
            valid_set(&route.allowed_tools, false)?;
            valid_set(&route.allowed_data_classes, false)?;
            valid_set(&route.required_host_capabilities, true)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        let mut hash = Sha256::new();
        part(&mut hash, MODEL_ROUTE_CATALOGUE_SCHEMA);
        number(&mut hash, self.version);
        let mut routes: Vec<_> = self.routes.iter().collect();
        routes.sort_by(|a, b| a.id.cmp(&b.id));
        number(&mut hash, routes.len() as u64);
        for route in routes {
            for value in [&route.id, &route.provider, &route.model, &route.effort] {
                part(&mut hash, value);
            }
            number(&mut hash, u64::from(route.enabled));
            let mut choices = route.allowed_matrix_choice_ids.clone();
            choices.sort();
            number(&mut hash, choices.len() as u64);
            for choice in choices {
                part(&mut hash, &choice);
            }
            for values in [
                &route.allowed_roles,
                &route.allowed_tools,
                &route.allowed_data_classes,
                &route.required_host_capabilities,
            ] {
                let mut sorted = values.clone();
                sorted.sort();
                number(&mut hash, sorted.len() as u64);
                for value in sorted {
                    part(&mut hash, &value);
                }
            }
            number(&mut hash, route.minimum_budget_units);
            number(&mut hash, route.minimum_latency_ms);
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    pub fn eligible(&self, work: &ModelRouteWorkContext) -> Result<EligibleModelRoutes> {
        let catalogue_digest = self.digest()?;
        let work_context_digest = work.digest()?;
        let (role, tool, data_class, capabilities, budget, latency) = match (
            &work.role,
            &work.tool,
            &work.data_class,
            &work.host_capabilities,
            &work.remaining_budget_units,
            &work.available_latency_ms,
        ) {
            (
                ModelRouteFact::Known { value: role, .. },
                ModelRouteFact::Known { value: tool, .. },
                ModelRouteFact::Known {
                    value: data_class, ..
                },
                ModelRouteFact::Known {
                    value: capabilities,
                    ..
                },
                ModelRouteFact::Known { value: budget, .. },
                ModelRouteFact::Known { value: latency, .. },
            ) => (role, tool, data_class, capabilities, budget, latency),
            _ => {
                let mut configured_route_ids: Vec<_> =
                    self.routes.iter().map(|route| route.id.clone()).collect();
                configured_route_ids.sort();
                return Ok(EligibleModelRoutes {
                    catalogue_version: self.version,
                    catalogue_digest,
                    work_context_digest,
                    configured_route_ids,
                    route_ids: Vec::new(),
                });
            }
        };
        let capabilities: BTreeSet<_> = capabilities.iter().collect();
        let mut configured_route_ids: Vec<_> =
            self.routes.iter().map(|route| route.id.clone()).collect();
        configured_route_ids.sort();
        let mut route_ids: Vec<_> = self
            .routes
            .iter()
            .filter(|route| {
                route.enabled
                    && route
                        .allowed_matrix_choice_ids
                        .contains(&work.approved_matrix_selection.selected_choice_id)
                    && route.allowed_roles.contains(role)
                    && route.allowed_tools.contains(tool)
                    && route.allowed_data_classes.contains(data_class)
                    && route
                        .required_host_capabilities
                        .iter()
                        .all(|capability| capabilities.contains(capability))
                    && *budget >= route.minimum_budget_units
                    && *latency >= route.minimum_latency_ms
            })
            .map(|route| route.id.clone())
            .collect();
        route_ids.sort();
        Ok(EligibleModelRoutes {
            catalogue_version: self.version,
            catalogue_digest,
            work_context_digest,
            configured_route_ids,
            route_ids,
        })
    }
}
