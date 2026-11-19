// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface ILinkEnd extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract com.change_vision.jude.api.inf.model.IInstanceSpecification getType();
    public abstract void setNavigability(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setAggregation(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setComposite(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setName(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
