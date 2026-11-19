// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IClass extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract com.change_vision.jude.api.inf.model.IClassifierTemplateParameter[] getTemplateParameters();
    public abstract void setAbstract(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setLeaf(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setActive(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setName(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
