// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface ITemplateBinding extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract com.change_vision.jude.api.inf.model.IClass getTemplate();
    public abstract void addActualParameter(com.change_vision.jude.api.inf.model.IClassifierTemplateParameter p0, java.lang.Object p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
